"""Answer one question: did PR #139 make streaming cheaper?

The change removes one copy of every streamed byte. Chunks used to be copied
twice (into a Vec<u8>, then into a Python bytes); now once. Everything here
exists to decide whether that is measurable.

Output leads with a VERDICT. The tables underneath are for when you want to
check the working, and are hidden unless --detail is passed.

Two design decisions are load-bearing and should not be "simplified" away:

  * PAIRED analysis. base and head run adjacent within a round, so differencing
    inside a round cancels machine drift. An unpaired test lost nearly all its
    power on a drifting box (see run_ab.sh for the ordering that makes this
    valid).
  * ABSOLUTE saving as the primary number. Removing one copy costs a fixed
    seconds-per-GB regardless of payload size, so the constant is the real
    quantity and the percentage is derived from it.

Throughput is deliberately not analyzed: at this effect size it was pure noise
and produced more confusion than signal. Raw records still carry mb_s if you
want it.
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
from dataclasses import asdict, dataclass, field

from records import Cell, RunRecord


class PairedPermutationTest:
    """Two-sided sign-flip test on per-round paired differences.

    Under the null the sign of each paired difference is exchangeable, so
    shuffling signs gives the reference distribution. Nonparametric, immune to
    session-long drift, stdlib only.
    """

    ITERATIONS = 20_000
    SEED = 20260723  # fixed so rerunning on the same raw data is reproducible
    ALPHA = 0.05

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
        # A p-value of exactly zero is not something finite resampling can
        # justify, hence the add-one correction.
        p = (extreme + 1) / (cls.ITERATIONS + 1)
        cls._cache[key] = p
        return p


@dataclass(frozen=True)
class Metric:
    key: str
    label: str
    unit: str
    fmt: str
    abs_fmt: str  # absolute deltas need finer precision than the level itself

    def value_of(self, record: RunRecord) -> float | None:
        return getattr(record, self.key)


CPU = Metric("cpu_s_per_gb", "CPU seconds per GB", "s/GB", "{:.3f}", "{:+.4f}")
RSS = Metric("max_rss_mb", "Peak RSS", "MB", "{:.1f}", "{:+.2f}")
METRICS: tuple[Metric, ...] = (CPU, RSS)


# --------------------------------------------------------------------------
# Rendering primitives — models produce rows, tables own all the formatting.
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Column:
    header: str
    align: str = ">"


@dataclass(frozen=True)
class Table:
    """Self-sizing text table. Widths come from the content, so no caller
    hardcodes a format string and no column silently truncates."""

    columns: tuple[Column, ...]
    rows: tuple[tuple[str, ...], ...]
    title: str = ""
    indent: str = ""
    GAP = 2

    def widths(self) -> list[int]:
        return [
            max([len(col.header)] + [len(r[i]) for r in self.rows])
            for i, col in enumerate(self.columns)
        ]

    def _line(self, cells: tuple[str, ...]) -> str:
        gap = " " * self.GAP
        parts = [
            f"{cell:{col.align}{width}}"
            for cell, col, width in zip(cells, self.columns, self.widths())
        ]
        return self.indent + gap.join(parts).rstrip()

    def render(self) -> str:
        header = self._line(tuple(c.header for c in self.columns))
        rule = self.indent + "-" * (len(header) - len(self.indent))
        body = [self._line(r) for r in self.rows]
        return "\n".join(([self.title] if self.title else []) + [header, rule] + body)


# --------------------------------------------------------------------------
# Measurements
# --------------------------------------------------------------------------


@dataclass
class Samples:
    """One metric, one cell, one arm — keyed by round so pairing is possible."""

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
    def median(self) -> float:
        return statistics.median(self.values) if self.by_round else 0.0

    @property
    def drift_pct(self) -> float:
        """Movement from the first third of the session to the last. A property
        of the machine, not of the code under test."""
        ordered = self.values
        third = len(ordered) // 3
        if third < 1:
            return 0.0
        first = statistics.median(ordered[:third])
        return (statistics.median(ordered[-third:]) - first) / first * 100 if first else 0.0

    def stats(self) -> ArmStats:
        vals = self.values
        return ArmStats(
            median=round(self.median, 4),
            minimum=round(min(vals), 4) if vals else 0.0,
            maximum=round(max(vals), 4) if vals else 0.0,
            rounds=self.rounds,
        )


@dataclass(frozen=True)
class ArmStats:
    median: float
    minimum: float
    maximum: float
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
        """Per-round relative differences. Differencing inside a round is what
        makes this drift-immune: both arms ran under the same conditions."""
        shared = sorted(set(self.base_samples.by_round) & set(self.head_samples.by_round))
        return [
            (self.head_samples.by_round[r] - base) / base * 100
            for r in shared
            if (base := self.base_samples.by_round[r])
        ]

    @property
    def delta_pct(self) -> float:
        deltas = self.paired_deltas
        return statistics.median(deltas) if deltas else 0.0

    @property
    def abs_delta(self) -> float:
        """Signed difference in the metric's own units. For CPU this is the
        quantity the mechanism predicts to be CONSTANT across every cell."""
        return self.head_samples.median - self.base_samples.median

    @property
    def p_value(self) -> float:
        return PairedPermutationTest.p_value(self.paired_deltas)

    @property
    def rounds(self) -> int:
        return len(self.paired_deltas)

    @property
    def improved(self) -> bool:
        return self.delta_pct < 0  # both remaining metrics are lower-is-better

    @property
    def verdict(self) -> str:
        if self.p_value >= PairedPermutationTest.ALPHA:
            return "noise"
        return "head better" if self.improved else "HEAD WORSE"

    def to_row(self) -> tuple[str, ...]:
        p = self.p_value
        return (
            self.cell.render(),
            self.metric.fmt.format(self.base.median),
            self.metric.fmt.format(self.head.median),
            self.metric.abs_fmt.format(self.abs_delta),
            f"{self.delta_pct:.1f}%",
            f"{p:.4f}" if p < 0.999 else ">0.999",
            self.verdict,
        )


@dataclass(frozen=True)
class MetricTable:
    metric: Metric
    comparisons: tuple[Comparison, ...]

    COLUMNS = (
        Column("cell", "<"),
        Column("base"),
        Column("head"),
        Column("abs d"),
        Column("delta"),
        Column("p"),
        Column("verdict", "<"),
    )

    def resolved(self) -> tuple[Comparison, ...]:
        return tuple(c for c in self.comparisons if c.verdict != "noise")

    def wins(self) -> tuple[Comparison, ...]:
        return tuple(c for c in self.resolved() if c.improved)

    def losses(self) -> tuple[Comparison, ...]:
        return tuple(c for c in self.resolved() if not c.improved)

    def best(self) -> Comparison | None:
        wins = self.wins()
        return min(wins, key=lambda c: c.delta_pct) if wins else None

    def to_table(self) -> Table:
        return Table(
            columns=self.COLUMNS,
            rows=tuple(c.to_row() for c in self.comparisons),
            title=f"{self.metric.label} ({self.metric.unit}) — lower is better",
        )

    def render(self) -> str:
        return self.to_table().render()


# --------------------------------------------------------------------------
# Diagnostics
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class MechanismCheck:
    """Does one constant explain every cell?

    Removing one copy costs a fixed amount per GB, so every cell should show
    the same ABSOLUTE saving and differ in percentage only because baselines
    differ. That makes the constant falsifiable rather than a story fitted
    afterwards — and it correctly predicts the cells where the effect is too
    small to see, which a fitted story would not.
    """

    table: MetricTable

    COLUMNS = (
        Column("cell", "<"),
        Column("base"),
        Column("implied"),
        Column("observed"),
        Column("verdict", "<"),
    )

    @property
    def constant(self) -> float | None:
        wins = self.table.wins()
        return statistics.median([c.abs_delta for c in wins]) if wins else None

    @property
    def us_per_mb(self) -> float | None:
        k = self.constant
        return abs(k) / 1024 * 1e6 if k is not None else None

    def implied_pct(self, c: Comparison) -> float | None:
        k = self.constant
        base = c.base.median
        return (k / base * 100) if (k is not None and base) else None

    def to_table(self) -> Table:
        rows = []
        for c in self.table.comparisons:
            implied = self.implied_pct(c)
            rows.append(
                (
                    c.cell.render(),
                    f"{c.base.median:.3f}",
                    f"{implied:.1f}%" if implied is not None else "-",
                    f"{c.delta_pct:.1f}%",
                    c.verdict,
                )
            )
        return Table(columns=self.COLUMNS, rows=tuple(rows), indent="  ")

    def render(self) -> str:
        k = self.constant
        if k is None:
            return (
                "MECHANISM CHECK\n"
                "  No cell resolved, so there is no constant to check against.\n"
            )
        return "\n".join(
            [
                "MECHANISM CHECK",
                f"  One constant of {k:+.4f} {self.table.metric.unit} "
                f"(~{self.us_per_mb:.0f} us/MB) should explain every cell.",
                "  'implied' is what that predicts here; 'observed' is measured.",
                "",
                self.to_table().render(),
                "",
            ]
        )


@dataclass(frozen=True)
class DriftReport:
    """Surfaces machine instability. Pairing stops drift biasing the verdicts,
    but a drifting session's absolute numbers are not comparable to another's."""

    table: MetricTable
    THRESHOLD = 10.0

    def drifting(self) -> list[tuple[Cell, float]]:
        return [
            (c.cell, c.base_samples.drift_pct)
            for c in self.table.comparisons
            if abs(c.base_samples.drift_pct) >= self.THRESHOLD
        ]

    @property
    def ok(self) -> bool:
        return not self.drifting()

    def render(self) -> str:
        drifting = self.drifting()
        if not drifting:
            return f"Machine drift: all cells under {self.THRESHOLD:.0f}%.\n"
        lines = [
            f"MACHINE DRIFT over {self.THRESHOLD:.0f}% (base arm, first third vs last):",
            "  Verdicts are paired so this does not bias them, but absolute",
            "  numbers are not comparable across sessions.",
        ]
        lines += [f"    {cell.render():<20}{pct:+.1f}%" for cell, pct in drifting]
        return "\n".join(lines) + "\n"


@dataclass(frozen=True)
class Verdict:
    """The answer, before any numbers. This is the part people read."""

    cpu: MetricTable
    rss: MetricTable
    drift: DriftReport
    rounds: int

    @property
    def headline(self) -> str:
        wins, losses = self.cpu.wins(), self.cpu.losses()
        if losses and wins:
            return "MIXED — some configs better, some worse"
        if losses:
            return "head is SLOWER"
        if wins:
            return "head is faster"
        return "no measurable difference"

    @staticmethod
    def plural(n: int, word: str) -> str:
        return f"{n} {word}" if n == 1 else f"{n} {word}s"

    @property
    def trust(self) -> str:
        problems = []
        if self.rounds < 6:
            problems.append(f"only {self.rounds} rounds — use 10 or more")
        if self.rounds % 2:
            problems.append("odd round count — arm order is not balanced")
        if not self.drift.ok:
            problems.append("machine drift over 10% — quiet the box and rerun")
        if problems:
            return "NO — " + "; ".join(problems)
        return f"yes — {self.rounds} paired rounds, arms alternated, drift under 10%"

    def lines(self) -> list[str]:
        out = [f"VERDICT  {self.headline}", ""]

        check = MechanismCheck(self.cpu)
        k, wins = check.constant, self.cpu.wins()
        total = len(self.cpu.comparisons)
        if k is not None:
            out += [
                f"  CPU   {k:+.3f} {CPU.unit} (~{check.us_per_mb:.0f} us/MB) "
                f"in {len(wins)} of {self.plural(total, 'config')}",
                "        one constant explains the rest, including the configs",
                "        where the effect is too small to measure",
            ]
        else:
            out.append(f"  CPU   no measurable change in {self.plural(total, 'config')}")

        best_rss = self.rss.best()
        if best_rss:
            out.append(
                f"  RSS   {best_rss.delta_pct:.0f}% at {best_rss.cell.render()} "
                f"({len(self.rss.wins())} of "
                f"{self.plural(len(self.rss.comparisons), 'config')} improved)"
            )
        else:
            out.append("  RSS   no measurable change")

        return out + ["", f"  Trustworthy: {self.trust}", ""]

    def render(self) -> str:
        return "\n".join(self.lines())


# --------------------------------------------------------------------------
# Report
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Meta:
    base_ref: str
    head_ref: str
    rounds: int
    toolchain: str
    question: str = "does removing the per-chunk double copy make streaming cheaper?"
    primary_metric: str = CPU.key
    method: str = (
        "paired per-round differences, two-sided sign-flip permutation test, "
        f"{PairedPermutationTest.ITERATIONS} resamples, alpha {PairedPermutationTest.ALPHA}"
    )


@dataclass(frozen=True)
class CellSummary:
    cell: str
    metrics: dict[str, dict]


@dataclass(frozen=True)
class Report:
    meta: Meta
    tables: tuple[MetricTable, ...]

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
                    if value is not None:
                        key = (record.cell, record.arm, metric.key)
                        samples.setdefault(key, Samples()).add(record.round, value)

        tables = []
        for metric in METRICS:
            comparisons = [
                Comparison(metric, cell, base, head)
                for cell in order
                if (base := samples.get((cell, "base", metric.key)))
                and (head := samples.get((cell, "head", metric.key)))
            ]
            tables.append(MetricTable(metric, tuple(comparisons)))
        return cls(meta, tuple(tables))

    def table_for(self, metric: Metric) -> MetricTable:
        return next(t for t in self.tables if t.metric.key == metric.key)

    @property
    def rounds(self) -> int:
        return max((c.rounds for t in self.tables for c in t.comparisons), default=0)

    def drift(self) -> DriftReport:
        return DriftReport(self.table_for(CPU))

    def verdict(self) -> Verdict:
        return Verdict(
            cpu=self.table_for(CPU),
            rss=self.table_for(RSS),
            drift=self.drift(),
            rounds=self.rounds,
        )

    def render(self, detail: bool = False) -> str:
        blocks = ["", self.verdict().render()]
        if not detail:
            blocks.append("Run with --detail for per-config tables and p-values.\n")
            return "\n".join(blocks)
        blocks += [t.render() + "\n" for t in self.tables]
        blocks += [MechanismCheck(self.table_for(CPU)).render(), self.drift().render()]
        return "\n".join(blocks)

    def cell_summaries(self) -> list[CellSummary]:
        by_cell: dict[str, dict] = {}
        for table in self.tables:
            for c in table.comparisons:
                by_cell.setdefault(c.cell.render(), {})[c.metric.key] = {
                    "verdict": c.verdict,
                    "delta_pct": round(c.delta_pct, 2),
                    "abs_delta": round(c.abs_delta, 5),
                    "p_value": round(c.p_value, 5),
                    "base": asdict(c.base),
                    "head": asdict(c.head),
                }
        return [CellSummary(cell, metrics) for cell, metrics in by_cell.items()]

    def to_json(self) -> str:
        check = MechanismCheck(self.table_for(CPU))
        verdict = self.verdict()
        return json.dumps(
            {
                "meta": asdict(self.meta),
                "verdict": {
                    "headline": verdict.headline,
                    "trustworthy": verdict.trust.startswith("yes"),
                    "trust_detail": verdict.trust,
                    "rounds": self.rounds,
                },
                "mechanism_check": {
                    "implied_constant": (
                        round(check.constant, 5) if check.constant is not None else None
                    ),
                    "implied_us_per_mb": (
                        round(check.us_per_mb, 1) if check.us_per_mb is not None else None
                    ),
                    "fitted_on_cells": len(self.table_for(CPU).wins()),
                },
                "cells": [asdict(c) for c in self.cell_summaries()],
            },
            indent=2,
        )


@dataclass(frozen=True)
class Cli:
    raw: str
    json_out: str | None
    detail: bool
    meta: Meta

    @classmethod
    def from_argv(cls) -> Cli:
        p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
        p.add_argument("raw", nargs="?", default="/results/raw.jsonl")
        p.add_argument("--json", dest="json_out", help="write the summary as JSON here")
        p.add_argument("--detail", action="store_true", help="show per-config tables")
        p.add_argument("--base-ref", default="")
        p.add_argument("--head-ref", default="")
        p.add_argument("--rounds", type=int, default=0)
        p.add_argument("--toolchain", default="")
        a = p.parse_args()
        return cls(
            raw=a.raw,
            json_out=a.json_out,
            detail=a.detail,
            meta=Meta(a.base_ref, a.head_ref, a.rounds, a.toolchain),
        )

    def run(self) -> None:
        report = Report.load(self.raw, self.meta)
        print(report.render(detail=self.detail))
        if self.json_out:
            with open(self.json_out, "w") as fh:
                fh.write(report.to_json())
            print(f"wrote {self.json_out}")


if __name__ == "__main__":
    Cli.from_argv().run()
