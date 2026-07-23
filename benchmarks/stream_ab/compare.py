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
import random
import statistics
from dataclasses import asdict, dataclass, field

from records import Cell, RunRecord


class PairedPermutationTest:
    """Two-sided sign-flip permutation test on per-round paired differences.

    This is the third significance rule this script has had, and the reasons
    the first two failed are worth keeping.

    1. Range overlap. Half-range is an extreme-value statistic, so it only
       grows as rounds are added and overlap becomes MORE likely with more
       data. Going 5 -> 10 rounds turned every verdict into "noise" while the
       deltas barely moved. A rule that weakens as evidence accumulates is
       backwards.

    2. Unpaired permutation. Correct for i.i.d. samples, but these are not
       i.i.d.: the box degrades over a long session (thermal throttling, VM
       state). Across one 15-round run the BASE arm's CPU/GB rose ~60% and its
       throughput halved. Interleaving arms cancels the bias between them, but
       each arm's samples still span the whole drift, so within-arm variance is
       dominated by drift and the test loses nearly all power.

    The harness already runs base and head adjacent inside each round, so the
    measurements are naturally PAIRED: both arms in round k see almost the same
    machine state. Differencing within a round cancels drift, and the residual
    is what we actually want to test. Under the null the sign of each paired
    difference is exchangeable, so shuffling signs gives the reference
    distribution. Nonparametric, drift-immune, stdlib only.
    """

    ITERATIONS = 20_000
    SEED = 20260723  # fixed so a rerun on the same raw data is reproducible
    ALPHA = 0.05

    # The verdict, the table cell and the JSON all ask for the same p-value.
    # Resampling three times would triple the runtime for no new answer.
    _cache: dict[tuple, float] = {}

    @classmethod
    def p_value(cls, diffs: list[float]) -> float:
        if len(diffs) < 3:
            return 1.0
        key = tuple(diffs)
        if key in cls._cache:
            return cls._cache[key]
        observed = abs(statistics.median(diffs))
        rng = random.Random(cls.SEED)
        extreme = 0
        for _ in range(cls.ITERATIONS):
            flipped = [d if rng.random() < 0.5 else -d for d in diffs]
            if abs(statistics.median(flipped)) >= observed - 1e-12:
                extreme += 1
        # Add-one correction: a p-value of exactly zero is not a thing a
        # finite resampling can justify.
        p = (extreme + 1) / (cls.ITERATIONS + 1)
        cls._cache[key] = p
        return p


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
    """Repeated measurements of one metric, for one cell, for one arm.

    Keyed by round rather than appended to a list, because the analysis is
    paired: round k of base must be matched against round k of head.
    """

    by_round: dict[int, float] = field(default_factory=dict)

    def add(self, round_: int, value: float) -> None:
        self.by_round[round_] = value

    @property
    def values(self) -> list[float]:
        return [self.by_round[r] for r in sorted(self.by_round)]

    @property
    def rounds(self) -> int:
        return len(self.by_round)

    @property
    def drift_pct(self) -> float:
        """How much this arm moved from the first third of the session to the
        last. Large values mean the box was not in a steady state, which is a
        property of the machine, not of the code under test."""
        ordered = self.values
        third = len(ordered) // 3
        if third < 1:
            return 0.0
        early, late = ordered[:third], ordered[-third:]
        first = statistics.median(early)
        return (statistics.median(late) - first) / first * 100 if first else 0.0

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
    def rel_iqr_pct(self) -> float:
        """Interquartile range as a percentage of the median.

        Reported for context, never for the verdict. Unlike half-range this is
        stable as rounds are added rather than growing with every new outlier,
        so it is comparable between a 5-round and a 20-round session.
        """
        if self.rounds < 4 or not self.median:
            return 0.0
        quartiles = statistics.quantiles(self.values, n=4, method="inclusive")
        return (quartiles[2] - quartiles[0]) / self.median * 100

    def stats(self) -> ArmStats:
        return ArmStats(
            median=round(self.median, 4),
            minimum=round(self.minimum, 4),
            maximum=round(self.maximum, 4),
            rel_iqr_pct=round(self.rel_iqr_pct, 2),
            rounds=self.rounds,
        )


@dataclass(frozen=True)
class ArmStats:
    """Frozen summary of one arm's samples — what lands in the JSON."""

    median: float
    minimum: float
    maximum: float
    rel_iqr_pct: float
    rounds: int


@dataclass(frozen=True)
class Comparison:
    """base vs head for one cell and one metric."""

    metric: Metric
    cell: Cell
    base_samples: Samples
    head_samples: Samples

    @property
    def base(self) -> ArmStats:
        return self.base_samples.stats()

    @property
    def head(self) -> ArmStats:
        return self.head_samples.stats()

    @property
    def paired_deltas(self) -> list[float]:
        """Per-round relative differences, head vs base.

        Differencing inside a round is what makes this drift-immune: both arms
        ran adjacent under the same machine conditions, so whatever the box was
        doing that round cancels.
        """
        shared = sorted(
            set(self.base_samples.by_round) & set(self.head_samples.by_round)
        )
        out = []
        for r in shared:
            base = self.base_samples.by_round[r]
            if base:
                out.append((self.head_samples.by_round[r] - base) / base * 100)
        return out

    @property
    def delta_pct(self) -> float:
        deltas = self.paired_deltas
        return statistics.median(deltas) if deltas else 0.0

    @property
    def p_value(self) -> float:
        return PairedPermutationTest.p_value(self.paired_deltas)

    @property
    def drift_pct(self) -> float:
        return self.base_samples.drift_pct

    @property
    def verdict(self) -> str:
        if self.p_value >= PairedPermutationTest.ALPHA:
            return "noise"
        return "head better" if self.metric.improved(self.delta_pct) else "HEAD WORSE"

    def to_table(self) -> list[str]:
        p = self.p_value
        return [
            self.cell.mode,
            self.cell.payload,
            str(self.cell.concurrency),
            self.metric.fmt.format(self.base.median),
            self.metric.fmt.format(self.head.median),
            f"{self.delta_pct:.1f}%",
            f"{p:.4f}" if p < 0.999 else ">0.999",
            self.verdict,
        ]

    def render(self) -> str:
        return MetricTable.LAYOUT.format(*self.to_table())


@dataclass(frozen=True)
class MetricTable:
    """One metric's table across every cell."""

    HEADERS = ("mode", "payload", "conc", "base", "head", "delta", "p", "verdict")
    LAYOUT = "{:<6}{:<9}{:>5}  {:>11}{:>11}{:>9}{:>9}  {:<12}"

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
    significance_rule: str = (
        "two-sided sign-flip permutation test on per-round PAIRED differences, "
        f"{PairedPermutationTest.ITERATIONS} resamples, significant at "
        f"p < {PairedPermutationTest.ALPHA}"
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
        "'delta' is the median PER-ROUND difference, head vs base. 'p' is a",
        "two-sided sign-flip test on those paired differences; verdict needs",
        "p < 0.05. Pairing within a round cancels machine drift, which an",
        "unpaired test cannot do and which dominated earlier sessions.",
    )

    DRIFT_THRESHOLD = 10.0

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
                    samples.setdefault(key, Samples()).add(record.round, value)

        tables = []
        for metric in METRICS:
            comparisons = []
            for cell in order:
                base = samples.get((cell, "base", metric.key))
                head = samples.get((cell, "head", metric.key))
                if base and head:
                    comparisons.append(Comparison(metric, cell, base, head))
            tables.append(MetricTable(metric, tuple(comparisons)))
        return cls(meta, tuple(tables))

    def to_table(self) -> list[tuple[str, list[list[str]]]]:
        return [(t.metric.label, t.to_table()) for t in self.tables]

    def drift_report(self) -> list[str]:
        """Surface machine instability instead of letting it hide in the data.

        Drift is a property of the box, not the change. Pairing removes its
        effect on the verdicts, but a heavily drifting session still means the
        absolute base/head numbers are not comparable to another session's.
        """
        primary = self.tables[0]
        drifting = [
            (c.cell, c.drift_pct)
            for c in primary.comparisons
            if abs(c.drift_pct) >= self.DRIFT_THRESHOLD
        ]
        if not drifting:
            return [f"Machine drift: all cells under {self.DRIFT_THRESHOLD:.0f}%.", ""]
        lines = [
            f"MACHINE DRIFT (base arm, {primary.metric.label}, first third vs last):",
            "  Verdicts are paired so this does not bias them, but absolute",
            "  numbers are not comparable across sessions.",
        ]
        lines += [f"    {cell.render():<22}{pct:+.1f}%" for cell, pct in drifting]
        return lines + [""]

    def render(self) -> str:
        return "\n".join(
            (
                "",
                *self.PREAMBLE,
                "",
                *(t.render() for t in self.tables),
                *self.drift_report(),
            )
        )

    def cell_summaries(self) -> list[CellSummary]:
        by_cell: dict[Cell, dict[str, dict]] = {}
        for table in self.tables:
            for c in table.comparisons:
                by_cell.setdefault(c.cell, {})[c.metric.key] = {
                    "verdict": c.verdict,
                    "delta_pct": round(c.delta_pct, 2),
                    "p_value": round(c.p_value, 5),
                    "significant_at": PairedPermutationTest.ALPHA,
                    "base_drift_pct": round(c.drift_pct, 2),
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
