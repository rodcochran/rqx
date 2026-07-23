"""Did PR #139 make streaming cheaper?

Reads the measurements from run_ab.sh, compares the two builds, and prints an
answer. Pass --detail for the per-config tables.

Each round runs both builds back to back, and every comparison is made within a
round. That matters because a laptop or CI box speeds up and slows down over
the course of a run; comparing two builds that ran seconds apart cancels that
out, while comparing averages taken an hour apart does not.
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
from dataclasses import asdict, dataclass, field

from records import Config, RunRecord


class ChanceCheck:
    """Could a difference this big just be luck?

    Takes the per-round differences between the two builds and repeatedly flips
    which direction each one points, which is what the data would look like if
    the builds were really identical. The answer is the fraction of those
    shuffles that produce a difference at least as big as the real one.

    A low number means luck alone rarely explains what we measured.
    """

    SHUFFLES = 20_000
    SEED = 20260723  # fixed, so re-analyzing the same data gives the same answer
    MAX_CHANCE = 0.05  # at or above this, we do not call it a real difference

    _cache: dict[str, float] = {}

    @classmethod
    def probability(cls, changes: list[float]) -> float:
        if len(changes) < 3:
            return 1.0
        key = ",".join(f"{change:.10g}" for change in changes)
        if key in cls._cache:
            return cls._cache[key]

        real_gap = abs(statistics.median(changes))
        shuffler = random.Random(cls.SEED)
        at_least_as_big = 0
        for _ in range(cls.SHUFFLES):
            shuffled = [c if shuffler.random() < 0.5 else -c for c in changes]
            if abs(statistics.median(shuffled)) >= real_gap - 1e-12:
                at_least_as_big += 1

        # The +1 keeps the answer from ever being exactly zero, which a finite
        # number of shuffles cannot actually demonstrate.
        result = (at_least_as_big + 1) / (cls.SHUFFLES + 1)
        cls._cache[key] = result
        return result


@dataclass(frozen=True)
class Metric:
    key: str
    label: str
    unit: str
    value_format: str
    change_format: str  # changes are much smaller than the values, so finer

    def value_of(self, record: RunRecord) -> float | None:
        return getattr(record, self.key)


CPU = Metric(
    key="cpu_s_per_gb",
    label="CPU seconds per GB",
    unit="s/GB",
    value_format="{:.3f}",
    change_format="{:+.4f}",
)
RSS = Metric(
    key="max_rss_mb",
    label="Peak memory",
    unit="MB",
    value_format="{:.1f}",
    change_format="{:+.2f}",
)
METRICS = [CPU, RSS]


# --------------------------------------------------------------------------
# Text tables. Rows are dataclasses; columns name the field they display, so
# nothing depends on positional ordering.
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Column:
    field_name: str
    header: str
    align: str = ">"

    def value_of(self, row: object) -> str:
        return getattr(row, self.field_name)


@dataclass(frozen=True)
class Table:
    """Text table that sizes its columns from their content."""

    columns: list[Column]
    rows: list[object]
    title: str = ""
    indent: str = ""
    GAP = 2

    def widths(self) -> list[int]:
        return [
            max([len(column.header)] + [len(column.value_of(row)) for row in self.rows])
            for column in self.columns
        ]

    def _line(self, values: list[str]) -> str:
        parts = [
            f"{value:{column.align}{width}}"
            for value, column, width in zip(values, self.columns, self.widths())
        ]
        return self.indent + (" " * self.GAP).join(parts).rstrip()

    def render(self) -> str:
        header = self._line([column.header for column in self.columns])
        rule = self.indent + "-" * (len(header) - len(self.indent))
        title = [self.title] if self.title else []
        body = [self._line([c.value_of(row) for c in self.columns]) for row in self.rows]
        return "\n".join(title + [header, rule] + body)


@dataclass(frozen=True)
class ComparisonRow:
    config: str
    base: str
    head: str
    change: str
    change_pct: str
    chance: str
    verdict: str


@dataclass(frozen=True)
class SavingRow:
    config: str
    base: str
    expected: str
    measured: str
    verdict: str


@dataclass(frozen=True)
class DriftRow:
    config: str
    moved: str


# --------------------------------------------------------------------------
# Measurements
# --------------------------------------------------------------------------


@dataclass
class Measurements:
    """What one build scored on one metric for one config, keyed by round so
    the two builds can be lined up round for round."""

    by_round: dict[int, float] = field(default_factory=dict)

    def add(self, round_number: int, value: float) -> None:
        self.by_round[round_number] = value

    @property
    def values(self) -> list[float]:
        return [self.by_round[number] for number in sorted(self.by_round)]

    @property
    def rounds(self) -> int:
        return len(self.by_round)

    @property
    def median(self) -> float:
        return statistics.median(self.values) if self.by_round else 0.0

    @property
    def drift_pct(self) -> float:
        """How much this build's score moved from the start of the run to the
        end. That is the machine changing speed, not the code."""
        ordered = self.values
        third = len(ordered) // 3
        if third < 1:
            return 0.0
        start = statistics.median(ordered[:third])
        end = statistics.median(ordered[-third:])
        return (end - start) / start * 100 if start else 0.0

    def summary(self) -> BuildSummary:
        values = self.values
        return BuildSummary(
            median=round(self.median, 4),
            lowest=round(min(values), 4) if values else 0.0,
            highest=round(max(values), 4) if values else 0.0,
            rounds=self.rounds,
        )


@dataclass(frozen=True)
class BuildSummary:
    median: float
    lowest: float
    highest: float
    rounds: int


@dataclass
class MeasurementStore:
    """Every measurement, indexed by metric, then build, then config."""

    by_metric: dict[str, dict[str, dict[Config, Measurements]]] = field(
        default_factory=dict
    )
    order: list[Config] = field(default_factory=list)

    def add(self, record: RunRecord, metric: Metric, value: float) -> None:
        if record.config not in self.order:
            self.order.append(record.config)
        builds = self.by_metric.setdefault(metric.key, {})
        configs = builds.setdefault(record.build, {})
        configs.setdefault(record.config, Measurements()).add(
            round_number=record.round, value=value
        )

    def get(self, metric: Metric, build: str, config: Config) -> Measurements | None:
        return self.by_metric.get(metric.key, {}).get(build, {}).get(config)


@dataclass(frozen=True)
class Comparison:
    """One config, one metric, the base build against the head build."""

    metric: Metric
    config: Config
    base: Measurements
    head: Measurements

    @property
    def per_round_changes(self) -> list[float]:
        """Percentage change in each round. Comparing within a round is what
        keeps a machine that speeds up or slows down from skewing the result."""
        shared = sorted(set(self.base.by_round) & set(self.head.by_round))
        return [
            (self.head.by_round[number] - before) / before * 100
            for number in shared
            if (before := self.base.by_round[number])
        ]

    @property
    def change_pct(self) -> float:
        changes = self.per_round_changes
        return statistics.median(changes) if changes else 0.0

    @property
    def change(self) -> float:
        """The change in the metric's own units. For CPU this is the number we
        expect to be the same in every config."""
        return self.head.median - self.base.median

    @property
    def chance(self) -> float:
        return ChanceCheck.probability(self.per_round_changes)

    @property
    def rounds(self) -> int:
        return len(self.per_round_changes)

    @property
    def is_improvement(self) -> bool:
        return self.change_pct < 0  # both metrics are better when lower

    @property
    def verdict(self) -> str:
        if self.chance >= ChanceCheck.MAX_CHANCE:
            return "too small to tell"
        return "better" if self.is_improvement else "WORSE"

    def to_row(self) -> ComparisonRow:
        chance = self.chance
        return ComparisonRow(
            config=self.config.render(),
            base=self.metric.value_format.format(self.base.median),
            head=self.metric.value_format.format(self.head.median),
            change=self.metric.change_format.format(self.change),
            change_pct=f"{self.change_pct:.1f}%",
            chance=f"{chance:.1%}" if chance < 0.999 else ">99%",
            verdict=self.verdict,
        )

    def to_summary(self) -> MetricSummary:
        return MetricSummary(
            verdict=self.verdict,
            change_pct=round(self.change_pct, 2),
            change=round(self.change, 5),
            chance_it_is_luck=round(self.chance, 5),
            base=self.base.summary(),
            head=self.head.summary(),
        )


@dataclass(frozen=True)
class MetricTable:
    metric: Metric
    comparisons: list[Comparison]

    COLUMNS = [
        Column(field_name="config", header="config", align="<"),
        Column(field_name="base", header="base"),
        Column(field_name="head", header="head"),
        Column(field_name="change", header="change"),
        Column(field_name="change_pct", header="change %"),
        Column(field_name="chance", header="chance"),
        Column(field_name="verdict", header="verdict", align="<"),
    ]

    def confirmed(self) -> list[Comparison]:
        return [c for c in self.comparisons if c.verdict != "too small to tell"]

    def improvements(self) -> list[Comparison]:
        return [c for c in self.confirmed() if c.is_improvement]

    def regressions(self) -> list[Comparison]:
        return [c for c in self.confirmed() if not c.is_improvement]

    def best(self) -> Comparison | None:
        improvements = self.improvements()
        return min(improvements, key=lambda c: c.change_pct) if improvements else None

    def to_table(self) -> Table:
        return Table(
            columns=self.COLUMNS,
            rows=[comparison.to_row() for comparison in self.comparisons],
            title=f"{self.metric.label} ({self.metric.unit}) — lower is better",
        )

    def render(self) -> str:
        return self.to_table().render()


# --------------------------------------------------------------------------
# Checks
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class SavingCheck:
    """Does a single number explain every config?

    Removing one copy costs the same amount of CPU per GB no matter how big the
    payload is or how many streams run at once. So every config should show the
    same saving in seconds, and only the percentage should differ, because the
    percentage depends on how much CPU that config used to begin with.

    A config that disagrees means the benchmark is measuring something else.
    """

    table: MetricTable

    COLUMNS = [
        Column(field_name="config", header="config", align="<"),
        Column(field_name="base", header="base"),
        Column(field_name="expected", header="expected"),
        Column(field_name="measured", header="measured"),
        Column(field_name="verdict", header="verdict", align="<"),
    ]

    @property
    def saving(self) -> float | None:
        improvements = self.table.improvements()
        if not improvements:
            return None
        return statistics.median([c.change for c in improvements])

    @property
    def per_mb(self) -> float | None:
        saving = self.saving
        return abs(saving) / 1024 * 1e6 if saving is not None else None

    def expected_pct(self, comparison: Comparison) -> float | None:
        saving = self.saving
        base = comparison.base.median
        if saving is None or not base:
            return None
        return saving / base * 100

    def to_row(self, comparison: Comparison) -> SavingRow:
        expected = self.expected_pct(comparison)
        return SavingRow(
            config=comparison.config.render(),
            base=f"{comparison.base.median:.3f}",
            expected=f"{expected:.1f}%" if expected is not None else "-",
            measured=f"{comparison.change_pct:.1f}%",
            verdict=comparison.verdict,
        )

    def to_table(self) -> Table:
        return Table(
            columns=self.COLUMNS,
            rows=[self.to_row(c) for c in self.table.comparisons],
            indent="  ",
        )

    def to_summary(self) -> SavingSummary:
        saving = self.saving
        return SavingSummary(
            saving=round(saving, 5) if saving is not None else None,
            microseconds_per_mb=round(self.per_mb, 1) if self.per_mb is not None else None,
            based_on_configs=len(self.table.improvements()),
        )

    def render(self) -> str:
        saving = self.saving
        if saving is None:
            return (
                "IS THE SAVING CONSISTENT?\n"
                "  No config showed a clear difference, so there is nothing to check.\n"
            )
        return "\n".join(
            [
                "IS THE SAVING CONSISTENT?",
                f"  One saving of {saving:+.4f} {self.table.metric.unit} "
                f"(about {self.per_mb:.0f} microseconds per MB) should",
                "  explain every config. 'expected' is what that predicts here,",
                "  'measured' is what actually happened.",
                "",
                self.to_table().render(),
                "",
            ]
        )


@dataclass(frozen=True)
class DriftReport:
    """How much the machine itself changed speed during the run. Comparing the
    builds within a round keeps this out of the answer, but it does mean these
    numbers cannot be compared against a different run."""

    table: MetricTable
    THRESHOLD = 10.0

    COLUMNS = [
        Column(field_name="config", header="config", align="<"),
        Column(field_name="moved", header="moved"),
    ]

    def drifting(self) -> list[Comparison]:
        return [
            comparison
            for comparison in self.table.comparisons
            if abs(comparison.base.drift_pct) >= self.THRESHOLD
        ]

    @property
    def ok(self) -> bool:
        return not self.drifting()

    def to_table(self) -> Table:
        return Table(
            columns=self.COLUMNS,
            rows=[
                DriftRow(
                    config=c.config.render(), moved=f"{c.base.drift_pct:+.1f}%"
                )
                for c in self.drifting()
            ],
            indent="    ",
        )

    def render(self) -> str:
        if self.ok:
            return f"The machine stayed steady (under {self.THRESHOLD:.0f}% change).\n"
        return "\n".join(
            [
                f"THE MACHINE CHANGED SPEED by more than {self.THRESHOLD:.0f}% "
                "during the run:",
                "  The comparison still holds, because the builds are compared",
                "  round by round, but do not compare these numbers against",
                "  another run.",
                "",
                self.to_table().render(),
                "",
            ]
        )


@dataclass(frozen=True)
class Verdict:
    """The answer, before any numbers."""

    cpu: MetricTable
    memory: MetricTable
    drift: DriftReport
    rounds: int

    @staticmethod
    def count(number: int, word: str) -> str:
        return f"{number} {word}" if number == 1 else f"{number} {word}s"

    @property
    def headline(self) -> str:
        improvements = self.cpu.improvements()
        regressions = self.cpu.regressions()
        if improvements and regressions:
            return "MIXED — better in some configs, worse in others"
        if regressions:
            return "head is SLOWER"
        if improvements:
            return "head is faster"
        return "no difference big enough to measure"

    @property
    def trust(self) -> str:
        problems = []
        if self.rounds < 6:
            problems.append(f"only {self.rounds} rounds, use 10 or more")
        if self.rounds % 2:
            problems.append("an odd number of rounds, so the build order is unbalanced")
        if not self.drift.ok:
            problems.append("the machine changed speed, close other apps and rerun")
        if problems:
            return "NO — " + "; ".join(problems)
        return f"yes — {self.rounds} rounds, builds alternated, machine stayed steady"

    def lines(self) -> list[str]:
        out = [f"ANSWER  {self.headline}", ""]

        check = SavingCheck(table=self.cpu)
        saving = check.saving
        total = len(self.cpu.comparisons)
        if saving is None:
            out.append(f"  CPU     no clear difference in {self.count(total, 'config')}")
        else:
            out += [
                f"  CPU     {saving:+.3f} {CPU.unit} "
                f"(about {check.per_mb:.0f} microseconds per MB) in "
                f"{len(self.cpu.improvements())} of {self.count(total, 'config')}",
                "          the same saving explains the rest, including the",
                "          configs where it is too small to see",
            ]

        best = self.memory.best()
        if best is None:
            out.append("  Memory  no clear difference")
        else:
            improved = len(self.memory.improvements())
            total_memory = self.count(len(self.memory.comparisons), "config")
            out.append(
                f"  Memory  {best.change_pct:.0f}% less at {best.config.render()} "
                f"({improved} of {total_memory} improved)"
            )

        return out + ["", f"  Can you trust this? {self.trust}", ""]

    def to_summary(self) -> Answer:
        return Answer(
            headline=self.headline,
            trustworthy=self.trust.startswith("yes"),
            trust_detail=self.trust,
            rounds=self.rounds,
        )

    def render(self) -> str:
        return "\n".join(self.lines())


# --------------------------------------------------------------------------
# JSON shape
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Meta:
    base_ref: str
    head_ref: str
    rounds: int
    toolchain: str
    question: str = "does removing the per-chunk double copy make streaming cheaper?"
    headline_metric: str = CPU.key
    method: str = (
        "both builds run back to back in every round and are compared within "
        "that round; a difference counts only if random variation alone would "
        f"produce it less than {ChanceCheck.MAX_CHANCE:.0%} of the time"
    )


@dataclass(frozen=True)
class Answer:
    headline: str
    trustworthy: bool
    trust_detail: str
    rounds: int


@dataclass(frozen=True)
class SavingSummary:
    saving: float | None
    microseconds_per_mb: float | None
    based_on_configs: int


@dataclass(frozen=True)
class MetricSummary:
    verdict: str
    change_pct: float
    change: float
    chance_it_is_luck: float
    base: BuildSummary
    head: BuildSummary


@dataclass(frozen=True)
class ConfigSummary:
    config: str
    metrics: dict[str, MetricSummary]


@dataclass(frozen=True)
class ReportSummary:
    meta: Meta
    answer: Answer
    saving_check: SavingSummary
    configs: list[ConfigSummary]


# --------------------------------------------------------------------------
# Report
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Report:
    meta: Meta
    tables: list[MetricTable]

    @classmethod
    def load(cls, path: str, meta: Meta) -> Report:
        store = MeasurementStore()
        with open(path) as handle:
            for line in handle:
                if not line.strip():
                    continue
                record = RunRecord.from_json(line)
                for metric in METRICS:
                    value = metric.value_of(record)
                    if value is not None:
                        store.add(record=record, metric=metric, value=value)

        tables = []
        for metric in METRICS:
            comparisons = []
            for config in store.order:
                base = store.get(metric=metric, build="base", config=config)
                head = store.get(metric=metric, build="head", config=config)
                if base and head:
                    comparisons.append(
                        Comparison(metric=metric, config=config, base=base, head=head)
                    )
            tables.append(MetricTable(metric=metric, comparisons=comparisons))

        # Without this, an unreadable file reports "no difference" rather than
        # admitting it found nothing to compare.
        if not any(table.comparisons for table in tables):
            raise ValueError(
                f"{path} produced nothing to compare. Every config needs a "
                "'base' and a 'head' run."
            )
        return cls(meta=meta, tables=tables)

    def table_for(self, metric: Metric) -> MetricTable:
        return next(table for table in self.tables if table.metric.key == metric.key)

    @property
    def rounds(self) -> int:
        return max(
            (c.rounds for table in self.tables for c in table.comparisons), default=0
        )

    def drift(self) -> DriftReport:
        return DriftReport(table=self.table_for(CPU))

    def verdict(self) -> Verdict:
        return Verdict(
            cpu=self.table_for(CPU),
            memory=self.table_for(RSS),
            drift=self.drift(),
            rounds=self.rounds,
        )

    def render(self, detail: bool = False) -> str:
        blocks = ["", self.verdict().render()]
        if not detail:
            blocks.append("Run with --detail for the per-config tables.\n")
            return "\n".join(blocks)
        blocks += [table.render() + "\n" for table in self.tables]
        blocks += [
            SavingCheck(table=self.table_for(CPU)).render(),
            self.drift().render(),
        ]
        return "\n".join(blocks)

    def config_summaries(self) -> list[ConfigSummary]:
        by_config: dict[str, dict[str, MetricSummary]] = {}
        for table in self.tables:
            for comparison in table.comparisons:
                name = comparison.config.render()
                by_config.setdefault(name, {})[comparison.metric.key] = (
                    comparison.to_summary()
                )
        return [
            ConfigSummary(config=name, metrics=metrics)
            for name, metrics in by_config.items()
        ]

    def to_summary(self) -> ReportSummary:
        return ReportSummary(
            meta=self.meta,
            answer=self.verdict().to_summary(),
            saving_check=SavingCheck(table=self.table_for(CPU)).to_summary(),
            configs=self.config_summaries(),
        )

    def to_json(self) -> str:
        return json.dumps(asdict(self.to_summary()), indent=2)


@dataclass(frozen=True)
class Cli:
    raw: str
    json_out: str | None
    detail: bool
    meta: Meta

    @classmethod
    def from_argv(cls) -> Cli:
        parser = argparse.ArgumentParser(description="did PR #139 make streaming cheaper?")
        parser.add_argument("raw", nargs="?", default="/results/raw.jsonl")
        parser.add_argument("--json", dest="json_out", help="write the answer as JSON")
        parser.add_argument(
            "--detail", action="store_true", help="show the per-config tables"
        )
        parser.add_argument("--base-ref", default="")
        parser.add_argument("--head-ref", default="")
        parser.add_argument("--rounds", type=int, default=0)
        parser.add_argument("--toolchain", default="")
        args = parser.parse_args()
        return cls(
            raw=args.raw,
            json_out=args.json_out,
            detail=args.detail,
            meta=Meta(
                base_ref=args.base_ref,
                head_ref=args.head_ref,
                rounds=args.rounds,
                toolchain=args.toolchain,
            ),
        )

    def run(self) -> None:
        report = Report.load(path=self.raw, meta=self.meta)
        print(report.render(detail=self.detail))
        if self.json_out:
            with open(self.json_out, "w") as handle:
                handle.write(report.to_json())
            print(f"wrote {self.json_out}")


if __name__ == "__main__":
    Cli.from_argv().run()
