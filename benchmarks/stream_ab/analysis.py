"""Turning raw records into one comparison per config and metric."""

from __future__ import annotations

import random
import statistics
from dataclasses import dataclass, field

from metrics import Metric, Metrics
from records import Config, RunRecord
from summary import BuildSummary, MetricSummary
from tables import Column, Section, Table

TOO_SMALL = "too small to tell"


class ChanceCheck:
    """How likely a difference this big is from random variation alone.

    Takes the per-round differences between the two builds and repeatedly flips
    which direction each one points, which is what the data would look like if
    the builds were really identical. The result is the fraction of those
    shuffles that produce a difference at least as big as the real one.
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

        # The +1 keeps the result from ever being exactly zero, which a finite
        # number of shuffles cannot demonstrate.
        result = (at_least_as_big + 1) / (cls.SHUFFLES + 1)
        cls._cache[key] = result
        return result


@dataclass
class Measurements:
    """One build's scores on one metric for one config, keyed by round so the
    two builds can be lined up round for round."""

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
        """This build's movement from the start of the run to the end. That is
        the machine changing speed, not the code."""
        ordered = self.values
        third = len(ordered) // 3
        if third < 1:
            return 0.0
        start = statistics.median(ordered[:third])
        end = statistics.median(ordered[-third:])
        return (end - start) / start * 100 if start else 0.0

    def to_summary(self) -> BuildSummary:
        values = self.values
        return BuildSummary(
            median=round(self.median, 4),
            lowest=round(min(values), 4) if values else 0.0,
            highest=round(max(values), 4) if values else 0.0,
            rounds=self.rounds,
        )


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
class ComparisonRow:
    config: str
    base: str
    head: str
    change: str
    change_pct: str
    chance: str
    verdict: str


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
        return self.change_pct < 0  # every metric here is better when lower

    @property
    def verdict(self) -> str:
        if self.chance >= ChanceCheck.MAX_CHANCE:
            return TOO_SMALL
        return "better" if self.is_improvement else "worse"

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
            base=self.base.to_summary(),
            head=self.head.to_summary(),
        )


@dataclass(frozen=True)
class MetricResults:
    """Every config's comparison for one metric."""

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

    @classmethod
    def build(cls, metric: Metric, store: MeasurementStore) -> MetricResults:
        comparisons = []
        for config in store.order:
            base = store.get(metric=metric, build="base", config=config)
            head = store.get(metric=metric, build="head", config=config)
            if base and head:
                comparisons.append(
                    Comparison(metric=metric, config=config, base=base, head=head)
                )
        return cls(metric=metric, comparisons=comparisons)

    @classmethod
    def load_all(cls, path: str) -> list[MetricResults]:
        """One table per metric, read from a raw records file."""
        store = MeasurementStore()
        with open(path) as handle:
            for line in handle:
                if not line.strip():
                    continue
                record = RunRecord.from_json(line)
                for metric in Metrics.ALL:
                    value = metric.value_of(record)
                    if value is not None:
                        store.add(record=record, metric=metric, value=value)

        tables = [cls.build(metric=metric, store=store) for metric in Metrics.ALL]
        # Without this an unreadable file reports "no difference" rather than
        # admitting it found nothing to compare.
        if not any(table.comparisons for table in tables):
            raise ValueError(
                f"{path} produced nothing to compare. Every config needs a "
                "'base' and a 'head' run."
            )
        return tables

    def confirmed(self) -> list[Comparison]:
        return [c for c in self.comparisons if c.verdict != TOO_SMALL]

    def improvements(self) -> list[Comparison]:
        return [c for c in self.confirmed() if c.is_improvement]

    def regressions(self) -> list[Comparison]:
        return [c for c in self.confirmed() if not c.is_improvement]

    def best(self) -> Comparison | None:
        improvements = self.improvements()
        return min(improvements, key=lambda c: c.change_pct) if improvements else None

    def to_section(self) -> Section:
        return Section(
            heading=f"{self.metric.label} ({self.metric.unit}) — lower is better",
            table=Table(
                columns=self.COLUMNS,
                rows=[comparison.to_row() for comparison in self.comparisons],
            ),
        )
