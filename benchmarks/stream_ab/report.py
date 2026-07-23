"""What gets printed, and the JSON that mirrors it.

Three sections: the answer, whether one saving figure explains every config,
and whether the machine held still. All headings are title case, all values
lowercase, so nothing changes shape depending on the result.
"""

from __future__ import annotations

import json
import statistics
from dataclasses import asdict, dataclass

from analysis import ChanceCheck, Comparison, MetricResults
from metrics import Metrics
from summary import (
    About,
    Answer,
    ConfigSummary,
    MetricSummary,
    ReportSummary,
    RunInfo,
    SavingSummary,
)
from tables import Column, Section, Table


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


@dataclass(frozen=True)
class SavingCheck:
    """Whether one saving figure explains every config.

    Removing one copy costs the same amount of CPU per GB no matter how big the
    payload is or how many streams run at once. So every config should show the
    same saving in seconds, and only the percentage should differ, because the
    percentage depends on how much CPU that config used to begin with.

    A config that disagrees means the benchmark is measuring something else.
    """

    results: MetricResults

    COLUMNS = [
        Column(field_name="config", header="config", align="<"),
        Column(field_name="base", header="base"),
        Column(field_name="expected", header="expected"),
        Column(field_name="measured", header="measured"),
        Column(field_name="verdict", header="verdict", align="<"),
    ]

    @property
    def saving(self) -> float | None:
        improvements = self.results.improvements()
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

    def to_section(self) -> Section:
        saving = self.saving
        if saving is None:
            return Section(
                heading="Saving consistency",
                lines=["  No config showed a clear difference, so there is "
                       "nothing to check."],
            )
        return Section(
            heading="Saving consistency",
            lines=[
                f"  One saving of {saving:+.4f} {self.results.metric.unit} "
                f"(about {self.per_mb:.0f} microseconds per MB)",
                "  should explain every config. 'expected' is what that "
                "predicts here,",
                "  'measured' is what actually happened.",
            ],
            table=Table(
                columns=self.COLUMNS,
                rows=[self.to_row(c) for c in self.results.comparisons],
                indent="  ",
            ),
        )

    def to_summary(self) -> SavingSummary:
        saving = self.saving
        return SavingSummary(
            saving=round(saving, 5) if saving is not None else None,
            microseconds_per_mb=(
                round(self.per_mb, 1) if self.per_mb is not None else None
            ),
            based_on_configs=len(self.results.improvements()),
        )


@dataclass(frozen=True)
class DriftReport:
    """The machine's own change in speed over the run.

    Comparing the builds within a round keeps this out of the answer, but it
    does mean these numbers cannot be compared against a different run.
    """

    results: MetricResults
    THRESHOLD = 10.0

    COLUMNS = [
        Column(field_name="config", header="config", align="<"),
        Column(field_name="moved", header="moved"),
    ]

    def drifting(self) -> list[Comparison]:
        return [
            comparison
            for comparison in self.results.comparisons
            if abs(comparison.base.drift_pct) >= self.THRESHOLD
        ]

    @property
    def steady(self) -> bool:
        return not self.drifting()

    def to_section(self) -> Section:
        if self.steady:
            return Section(
                heading="Machine stability",
                lines=[f"  Steady, under {self.THRESHOLD:.0f}% change during "
                       "the run."],
            )
        return Section(
            heading="Machine stability",
            lines=[
                f"  Speed changed by more than {self.THRESHOLD:.0f}% during the "
                "run. The comparison",
                "  still holds, because the builds are compared round by round, "
                "but do",
                "  not compare these numbers against another run.",
            ],
            table=Table(
                columns=self.COLUMNS,
                rows=[
                    DriftRow(
                        config=c.config.render(), moved=f"{c.base.drift_pct:+.1f}%"
                    )
                    for c in self.drifting()
                ],
                indent="  ",
            ),
        )


@dataclass(frozen=True)
class Verdict:
    """The answer, before any numbers."""

    cpu: MetricResults
    memory: MetricResults
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
            return "mixed — faster in some configs, slower in others"
        if regressions:
            return "head is slower"
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
        if not self.drift.steady:
            problems.append("the machine changed speed, close other apps and rerun")
        if problems:
            return "no — " + "; ".join(problems)
        return f"yes — {self.rounds} rounds, builds alternated, machine stayed steady"

    def cpu_lines(self) -> list[str]:
        check = SavingCheck(results=self.cpu)
        saving = check.saving
        total = self.count(len(self.cpu.comparisons), "config")
        if saving is None:
            return [f"  CPU     no clear difference in {total}"]
        return [
            f"  CPU     {saving:+.3f} {Metrics.CPU.unit} "
            f"(about {check.per_mb:.0f} microseconds per MB) in "
            f"{len(self.cpu.improvements())} of {total}",
            "          the same saving explains the rest, including the configs",
            "          where it is too small to see",
        ]

    def memory_lines(self) -> list[str]:
        best = self.memory.best()
        if best is None:
            return ["  Memory  no clear difference"]
        total = self.count(len(self.memory.comparisons), "config")
        return [
            f"  Memory  {best.change_pct:.0f}% less at {best.config.render()} "
            f"({len(self.memory.improvements())} of {total} improved)"
        ]

    def to_summary(self) -> Answer:
        return Answer(
            headline=self.headline,
            trustworthy=self.trust.startswith("yes"),
            trust_detail=self.trust,
            rounds=self.rounds,
        )

    def to_section(self) -> Section:
        return Section(
            heading=f"ANSWER  {self.headline}",
            lines=[""] + self.cpu_lines() + self.memory_lines()
            + [f"  Trust   {self.trust}"],
        )


@dataclass(frozen=True)
class Report:
    run: RunInfo
    tables: list[MetricResults]

    QUESTION = "does removing the per-chunk double copy make streaming cheaper?"

    @classmethod
    def load(cls, path: str, run: RunInfo) -> Report:
        return cls(run=run, tables=MetricResults.load_all(path))

    def table_for(self, key: str) -> MetricResults:
        return next(table for table in self.tables if table.metric.key == key)

    @property
    def rounds(self) -> int:
        return max(
            (c.rounds for table in self.tables for c in table.comparisons), default=0
        )

    def drift(self) -> DriftReport:
        return DriftReport(results=self.table_for(Metrics.CPU.key))

    def saving_check(self) -> SavingCheck:
        return SavingCheck(results=self.table_for(Metrics.CPU.key))

    def verdict(self) -> Verdict:
        return Verdict(
            cpu=self.table_for(Metrics.CPU.key),
            memory=self.table_for(Metrics.MEMORY.key),
            drift=self.drift(),
            rounds=self.rounds,
        )

    def about(self) -> About:
        return About(
            question=self.QUESTION,
            headline_metric=Metrics.CPU.key,
            method=(
                "both builds run back to back in every round and are compared "
                "within that round; a difference counts only if random "
                "variation alone would produce it less than "
                f"{ChanceCheck.MAX_CHANCE:.0%} of the time"
            ),
        )

    def sections(self, detail: bool) -> list[Section]:
        parts = [self.verdict().to_section()]
        if not detail:
            parts.append(
                Section(heading="Run with --detail for the per-config tables.")
            )
            return parts
        parts += [results.to_section() for results in self.tables]
        parts += [self.saving_check().to_section(), self.drift().to_section()]
        return parts

    def render(self, detail: bool = False) -> str:
        return "\n" + "\n".join(
            section.render() for section in self.sections(detail=detail)
        )

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
            run=self.run,
            about=self.about(),
            answer=self.verdict().to_summary(),
            saving_check=self.saving_check().to_summary(),
            configs=self.config_summaries(),
        )

    def to_json(self) -> str:
        return json.dumps(asdict(self.to_summary()), indent=2)
