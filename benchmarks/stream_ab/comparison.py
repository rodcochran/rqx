"""Comparing one config between the two builds.

Each round runs both builds back to back, so every comparison is made within a
round. That matters because a machine speeds up and slows down over a long run;
two builds that ran seconds apart saw the same conditions.
"""

from __future__ import annotations

import random
import statistics
from dataclasses import dataclass, field

from tables import Column, Table

TOO_SMALL = "too small to tell"


@dataclass(frozen=True)
class Metric:
    key: str
    label: str
    unit: str
    value_format: str
    change_format: str
    # True when the change should be the same number in every config. Removing
    # one copy costs a fixed amount of CPU per GB; memory savings scale with
    # how much is in flight, so only CPU gets checked against a constant.
    constant_change: bool


class Metrics:
    CPU = Metric(
        key="cpu_s_per_gb",
        label="CPU seconds per GB",
        unit="s/GB",
        value_format="{:.3f}",
        change_format="{:+.4f}",
        constant_change=True,
    )
    MEMORY = Metric(
        key="max_rss_mb",
        label="Peak memory",
        unit="MB",
        value_format="{:.1f}",
        change_format="{:+.2f}",
        constant_change=False,
    )
    ALL = [CPU, MEMORY]


class ChanceCheck:
    """How likely a difference this big is from random variation alone.

    Repeatedly flips which direction each per-round difference points, which is
    what the data would look like if the builds were identical, and returns the
    fraction of those shuffles that reach the real difference.
    """

    SHUFFLES = 20_000
    SEED = 20260723  # fixed, so re-analyzing the same data gives the same answer
    MAX_CHANCE = 0.05

    _cache: dict[str, float] = {}

    @classmethod
    def probability(cls, changes: list[float]) -> float:
        if len(changes) < 3:
            return 1.0
        key = ",".join(f"{change:.10g}" for change in changes)
        if key not in cls._cache:
            target = abs(statistics.median(changes))
            shuffler = random.Random(cls.SEED)
            hits = sum(
                abs(
                    statistics.median(
                        [c if shuffler.random() < 0.5 else -c for c in changes]
                    )
                )
                >= target - 1e-12
                for _ in range(cls.SHUFFLES)
            )
            # The +1 stops the result being exactly zero, which a finite number
            # of shuffles cannot demonstrate.
            cls._cache[key] = (hits + 1) / (cls.SHUFFLES + 1)
        return cls._cache[key]


@dataclass
class Measurements:
    """One build's scores for one config and metric, keyed by round so the two
    builds can be lined up round for round."""

    by_round: dict[int, float] = field(default_factory=dict)

    @property
    def values(self) -> list[float]:
        return [self.by_round[number] for number in sorted(self.by_round)]

    @property
    def median(self) -> float:
        return statistics.median(self.values) if self.by_round else 0.0

    @property
    def drift_pct(self) -> float:
        """Movement from the start of the run to the end: the machine changing
        speed, not the code."""
        ordered = self.values
        third = len(ordered) // 3
        if third < 1:
            return 0.0
        start = statistics.median(ordered[:third])
        end = statistics.median(ordered[-third:])
        return (end - start) / start * 100 if start else 0.0


@dataclass(frozen=True)
class ResultRow:
    config: str
    base: str
    head: str
    change: str
    change_pct: str
    expected: str
    chance: str
    verdict: str


@dataclass(frozen=True)
class Comparison:
    """One config and one metric, base build against head build."""

    metric: Metric
    config: str
    base: Measurements
    head: Measurements

    @property
    def per_round_changes(self) -> list[float]:
        shared = sorted(set(self.base.by_round) & set(self.head.by_round))
        return [
            (self.head.by_round[number] - before) / before * 100
            for number in shared
            if (before := self.base.by_round[number])
        ]

    @property
    def rounds(self) -> int:
        return len(self.per_round_changes)

    @property
    def change_pct(self) -> float:
        changes = self.per_round_changes
        return statistics.median(changes) if changes else 0.0

    @property
    def change(self) -> float:
        return self.head.median - self.base.median

    @property
    def chance(self) -> float:
        return ChanceCheck.probability(self.per_round_changes)

    @property
    def is_improvement(self) -> bool:
        return self.change_pct < 0  # every metric here is better when lower

    @property
    def verdict(self) -> str:
        if self.chance >= ChanceCheck.MAX_CHANCE:
            return TOO_SMALL
        return "better" if self.is_improvement else "worse"

    def expected_pct(self, saving: float | None) -> float | None:
        if saving is None or not self.base.median:
            return None
        return saving / self.base.median * 100

    def to_row(self, saving: float | None) -> ResultRow:
        expected = self.expected_pct(saving)
        return ResultRow(
            config=self.config,
            base=self.metric.value_format.format(self.base.median),
            head=self.metric.value_format.format(self.head.median),
            change=self.metric.change_format.format(self.change),
            change_pct=f"{self.change_pct:.1f}%",
            expected=f"{expected:.1f}%" if expected is not None else "-",
            chance=f"{self.chance:.1%}" if self.chance < 0.999 else ">99%",
            verdict=self.verdict,
        )

    def to_dict(self) -> dict:
        return {
            "config": self.config,
            "metric": self.metric.key,
            "verdict": self.verdict,
            "base": round(self.base.median, 4),
            "head": round(self.head.median, 4),
            "change": round(self.change, 5),
            "change_pct": round(self.change_pct, 2),
            "chance_it_is_luck": round(self.chance, 5),
            "rounds": self.rounds,
        }


@dataclass(frozen=True)
class MetricResults:
    """Every config's comparison for one metric."""

    metric: Metric
    comparisons: list[Comparison]

    BASE_COLUMNS = [
        Column(field_name="config", header="config", align="<"),
        Column(field_name="base", header="base"),
        Column(field_name="head", header="head"),
        Column(field_name="change", header="change"),
        Column(field_name="change_pct", header="change %"),
    ]
    TAIL_COLUMNS = [
        Column(field_name="chance", header="chance"),
        Column(field_name="verdict", header="verdict", align="<"),
    ]
    EXPECTED_COLUMN = Column(field_name="expected", header="expected")

    def improvements(self) -> list[Comparison]:
        return [c for c in self.comparisons if c.verdict == "better"]

    def regressions(self) -> list[Comparison]:
        return [c for c in self.comparisons if c.verdict == "worse"]

    def best(self) -> Comparison | None:
        wins = self.improvements()
        return min(wins, key=lambda c: c.change_pct) if wins else None

    @property
    def saving(self) -> float | None:
        """The one change that should explain every config, taken from those
        that showed a clear difference."""
        if not self.metric.constant_change:
            return None
        wins = self.improvements()
        return statistics.median([c.change for c in wins]) if wins else None

    @property
    def per_mb(self) -> float | None:
        return abs(self.saving) / 1024 * 1e6 if self.saving is not None else None

    def columns(self) -> list[Column]:
        middle = [self.EXPECTED_COLUMN] if self.metric.constant_change else []
        return self.BASE_COLUMNS + middle + self.TAIL_COLUMNS

    def render(self) -> str:
        table = Table(
            columns=self.columns(),
            rows=[c.to_row(self.saving) for c in self.comparisons],
        )
        lines = [
            f"{self.metric.label} ({self.metric.unit}) — lower is better",
            table.render(),
        ]
        if self.metric.constant_change and self.saving is not None:
            lines.append(
                "expected = what the single saving above predicts for this "
                "config's baseline"
            )
        return "\n".join(lines) + "\n"
