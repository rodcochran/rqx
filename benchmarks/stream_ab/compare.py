"""Aggregate the A/B records and report whether each delta clears the noise.

Every metric gets the SAME table shape and the SAME significance rule:

    mode | payload | conc | base | head | delta | noise | verdict

Reporting RSS or throughput without a verdict column invites reading a 1%
median difference as a result. Applying the rule uniformly makes the
untrustworthy columns visibly untrustworthy instead of relying on prose.

The change under test removes roughly one malloc + one memcpy per chunk — on
the order of tens of microseconds per MB. That is small, so a bare "head is 3%
better" is meaningless unless 3% exceeds run-to-run spread. Stdlib only.
"""

from __future__ import annotations

import argparse
import json
import statistics
from dataclasses import asdict, dataclass, field

from records import Cell, RunRecord


@dataclass(frozen=True)
class Metric:
    """A measured quantity, plus which direction counts as an improvement."""

    key: str
    label: str
    better: str  # "lower" | "higher"
    fmt: str

    def improved(self, delta_pct: float) -> bool:
        return delta_pct < 0 if self.better == "lower" else delta_pct > 0

    def value_of(self, record: RunRecord) -> float | None:
        return getattr(record, self.key)

    def render_heading(self) -> str:
        return f"{self.label}  ({self.better} is better)"


METRICS: tuple[Metric, ...] = (
    Metric("cpu_s_per_gb", "CPU seconds per GB streamed", "lower", "{:.3f}"),
    Metric("mb_s", "Throughput MB/s", "higher", "{:.1f}"),
    Metric("max_rss_mb", "Peak RSS MB", "lower", "{:.1f}"),
)


@dataclass
class Samples:
    """Repeated measurements of one metric, for one cell, for one arm."""

    values: list[float] = field(default_factory=list)

    def add(self, value: float) -> None:
        self.values.append(value)

    @property
    def rounds(self) -> int:
        return len(self.values)

    @property
    def median(self) -> float:
        return statistics.median(self.values) if self.values else 0.0

    @property
    def minimum(self) -> float:
        return min(self.values) if self.values else 0.0

    @property
    def maximum(self) -> float:
        return max(self.values) if self.values else 0.0

    @property
    def spread_pct(self) -> float:
        """Half-range as a percentage of the median — a blunt but honest
        stand-in for variability at small round counts, where stdev is itself
        poorly estimated."""
        if self.rounds < 2 or not self.median:
            return 0.0
        return (self.maximum - self.minimum) / 2 / self.median * 100

    def stats(self) -> ArmStats:
        return ArmStats(
            median=round(self.median, 4),
            minimum=round(self.minimum, 4),
            maximum=round(self.maximum, 4),
            spread_pct=round(self.spread_pct, 2),
            rounds=self.rounds,
        )


@dataclass(frozen=True)
class ArmStats:
    """Frozen summary of one arm's samples — what lands in the JSON."""

    median: float
    minimum: float
    maximum: float
    spread_pct: float
    rounds: int

    def overlaps(self, other: ArmStats) -> bool:
        return self.minimum <= other.maximum and other.minimum <= self.maximum


@dataclass(frozen=True)
class Comparison:
    """base vs head for one cell and one metric."""

    metric: Metric
    cell: Cell
    base: ArmStats
    head: ArmStats

    @property
    def delta_pct(self) -> float:
        if not self.base.median:
            return 0.0
        return (self.head.median - self.base.median) / self.base.median * 100

    @property
    def noise_pct(self) -> float:
        return max(self.base.spread_pct, self.head.spread_pct)

    @property
    def verdict(self) -> str:
        # A result must clear BOTH tests, because each alone is fooled:
        #
        #   1. Ranges must not overlap. Overlap means the rounds did not
        #      separate the arms at all.
        #   2. |delta| must exceed the spread. Non-overlap can happen by luck
        #      when one arm's distribution is skewed (median sitting near an
        #      extreme), which otherwise lets a ~1% delta pass as a finding.
        #
        # Requiring both costs the occasional real-but-marginal win. That is
        # the right trade for a change whose whole claim is a few percent.
        if self.base.overlaps(self.head) or abs(self.delta_pct) <= self.noise_pct:
            return "noise"
        return "head better" if self.metric.improved(self.delta_pct) else "HEAD WORSE"

    def to_table(self) -> list[str]:
        return [
            self.cell.mode,
            self.cell.payload,
            str(self.cell.concurrency),
            self.metric.fmt.format(self.base.median),
            self.metric.fmt.format(self.head.median),
            f"{self.delta_pct:.1f}%",
            f"{self.noise_pct:.1f}%",
            self.verdict,
        ]

    def render(self) -> str:
        return MetricTable.LAYOUT.format(*self.to_table())


@dataclass(frozen=True)
class MetricTable:
    """One metric's table across every cell."""

    HEADERS = ("mode", "payload", "conc", "base", "head", "delta", "noise", "verdict")
    LAYOUT = "{:<6}{:<9}{:>5}  {:>11}{:>11}{:>9}{:>8}  {:<12}"

    metric: Metric
    comparisons: tuple[Comparison, ...]

    def to_table(self) -> list[list[str]]:
        return [list(self.HEADERS)] + [c.to_table() for c in self.comparisons]

    def render(self) -> str:
        header = self.LAYOUT.format(*self.HEADERS)
        lines = [self.metric.render_heading(), header, "-" * len(header)]
        lines += [c.render() for c in self.comparisons]
        return "\n".join(lines) + "\n"


@dataclass(frozen=True)
class Meta:
    base_ref: str
    head_ref: str
    rounds: int
    toolchain: str
    primary_metric: str = "cpu_s_per_gb"
    noise_rule: str = (
        "verdict requires non-overlapping ranges AND |delta| greater than spread"
    )


@dataclass(frozen=True)
class CellSummary:
    """A cell's verdict across every metric — the JSON's unit of output."""

    mode: str
    payload: str
    concurrency: int
    metrics: dict[str, dict]


@dataclass(frozen=True)
class Report:
    meta: Meta
    tables: tuple[MetricTable, ...]

    PREAMBLE = (
        "Streaming A/B — issue #108 / PR #139",
        "",
        "PRIMARY metric is CPU seconds per GB. The change removes CPU and",
        "allocator work, so throughput and RSS are secondary — if they",
        "disagree with CPU time, believe CPU time.",
        "",
        "'delta' is head vs base. A verdict requires the arms' observed ranges",
        "to not overlap AND the delta to exceed the spread; otherwise 'noise'.",
    )

    @classmethod
    def load(cls, path: str, meta: Meta) -> Report:
        samples: dict[tuple[Cell, str, str], Samples] = {}
        order: list[Cell] = []
        with open(path) as fh:
            for line in fh:
                if not line.strip():
                    continue
                record = RunRecord.from_json(line)
                if record.cell not in order:
                    order.append(record.cell)
                for metric in METRICS:
                    value = metric.value_of(record)
                    if value is None:
                        continue
                    key = (record.cell, record.arm, metric.key)
                    samples.setdefault(key, Samples()).add(value)

        tables = []
        for metric in METRICS:
            comparisons = []
            for cell in order:
                base = samples.get((cell, "base", metric.key))
                head = samples.get((cell, "head", metric.key))
                if base and head:
                    comparisons.append(
                        Comparison(metric, cell, base.stats(), head.stats())
                    )
            tables.append(MetricTable(metric, tuple(comparisons)))
        return cls(meta, tuple(tables))

    def to_table(self) -> list[tuple[str, list[list[str]]]]:
        return [(t.metric.label, t.to_table()) for t in self.tables]

    def render(self) -> str:
        return "\n".join(("", *self.PREAMBLE, "", *(t.render() for t in self.tables)))

    def cell_summaries(self) -> list[CellSummary]:
        by_cell: dict[Cell, dict[str, dict]] = {}
        for table in self.tables:
            for c in table.comparisons:
                by_cell.setdefault(c.cell, {})[c.metric.key] = {
                    "verdict": c.verdict,
                    "delta_pct": round(c.delta_pct, 2),
                    "noise_pct": round(c.noise_pct, 2),
                    "base": asdict(c.base),
                    "head": asdict(c.head),
                }
        return [
            CellSummary(cell.mode, cell.payload, cell.concurrency, metrics)
            for cell, metrics in by_cell.items()
        ]

    def totals(self) -> dict[str, dict[str, int]]:
        counts: dict[str, dict[str, int]] = {}
        for table in self.tables:
            bucket = counts.setdefault(table.metric.key, {})
            for c in table.comparisons:
                bucket[c.verdict] = bucket.get(c.verdict, 0) + 1
        return counts

    def to_json(self) -> str:
        return json.dumps(
            {
                "meta": asdict(self.meta),
                "totals": self.totals(),
                "cells": [asdict(c) for c in self.cell_summaries()],
            },
            indent=2,
        )


@dataclass(frozen=True)
class Cli:
    raw: str
    json_out: str | None
    meta: Meta

    @classmethod
    def from_argv(cls) -> Cli:
        p = argparse.ArgumentParser()
        p.add_argument("raw", nargs="?", default="/results/raw.jsonl")
        p.add_argument("--json", dest="json_out", help="write the summary as JSON here")
        p.add_argument("--base-ref", default="")
        p.add_argument("--head-ref", default="")
        p.add_argument("--rounds", type=int, default=0)
        p.add_argument("--toolchain", default="")
        a = p.parse_args()
        return cls(
            raw=a.raw,
            json_out=a.json_out,
            meta=Meta(a.base_ref, a.head_ref, a.rounds, a.toolchain),
        )

    def run(self) -> None:
        report = Report.load(self.raw, self.meta)
        print(report.render())
        if self.json_out:
            with open(self.json_out, "w") as fh:
                fh.write(report.to_json())
            print(f"wrote {self.json_out}")


if __name__ == "__main__":
    Cli.from_argv().run()
