"""Compares the two builds and prints the answer.

Each round runs both builds back to back, so every comparison is made within a
round. That matters because a machine speeds up and slows down over a long run;
two builds that ran seconds apart saw the same conditions.
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
from dataclasses import dataclass, field

from records import RunRecord
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


@dataclass(frozen=True)
class Report:
    results: list[MetricResults]

    @classmethod
    def load(cls, path: str) -> Report:
        collected: dict[tuple[str, str, str], Measurements] = {}
        order: list[str] = []
        with open(path) as handle:
            for line in handle:
                if not line.strip():
                    continue
                record = RunRecord.from_json(line)
                if record.config not in order:
                    order.append(record.config)
                for metric in Metrics.ALL:
                    key = (metric.key, record.build, record.config)
                    measurements = collected.setdefault(key, Measurements())
                    measurements.by_round[record.round] = getattr(record, metric.key)

        results = []
        for metric in Metrics.ALL:
            comparisons = [
                Comparison(metric=metric, config=config, base=base, head=head)
                for config in order
                if (base := collected.get((metric.key, "base", config)))
                and (head := collected.get((metric.key, "head", config)))
            ]
            results.append(MetricResults(metric=metric, comparisons=comparisons))

        if not any(result.comparisons for result in results):
            raise SystemExit(
                f"{path} has nothing to compare; every config needs a base and "
                "a head run."
            )
        return cls(results=results)

    def for_metric(self, metric: Metric) -> MetricResults:
        return next(r for r in self.results if r.metric.key == metric.key)

    @property
    def cpu(self) -> MetricResults:
        return self.for_metric(Metrics.CPU)

    @property
    def memory(self) -> MetricResults:
        return self.for_metric(Metrics.MEMORY)

    @property
    def rounds(self) -> int:
        return max(c.rounds for r in self.results for c in r.comparisons)

    @property
    def worst_drift(self) -> float:
        return max(
            abs(c.base.drift_pct) for r in self.results for c in r.comparisons
        )

    @property
    def headline(self) -> str:
        better, worse = self.cpu.improvements(), self.cpu.regressions()
        if better and worse:
            return "mixed — faster in some configs, slower in others"
        if worse:
            return "head is slower"
        if better:
            return "head is faster"
        return "no difference big enough to measure"

    @property
    def problems(self) -> list[str]:
        found = []
        if self.rounds < 6:
            found.append(f"only {self.rounds} rounds, use 10 or more")
        if self.rounds % 2:
            found.append("an odd number of rounds, so the build order is unbalanced")
        if self.worst_drift >= 10:
            found.append(
                f"the machine changed speed by {self.worst_drift:.0f}%, "
                "close other apps and rerun"
            )
        return found

    @property
    def trust(self) -> str:
        if self.problems:
            return "no — " + "; ".join(self.problems)
        return (
            f"yes — {self.rounds} rounds, builds alternated, machine steady "
            f"(largest drift {self.worst_drift:.0f}%)"
        )

    def summary_lines(self) -> list[str]:
        cpu, memory = self.cpu, self.memory
        total = len(cpu.comparisons)
        lines = [f"ANSWER  {self.headline}", ""]

        if cpu.saving is None:
            lines.append(f"  CPU     no clear difference in {total} configs")
        else:
            lines.append(
                f"  CPU     {cpu.saving:+.3f} {Metrics.CPU.unit} "
                f"(about {cpu.per_mb:.0f} microseconds per MB) in "
                f"{len(cpu.improvements())} of {total} configs"
            )

        best = memory.best()
        if best is None:
            lines.append("  Memory  no clear difference")
        else:
            lines.append(
                f"  Memory  {best.change_pct:.0f}% at {best.config} "
                f"({len(memory.improvements())} of "
                f"{len(memory.comparisons)} configs improved)"
            )

        return lines + [f"  Trust   {self.trust}", ""]

    def render(self) -> str:
        blocks = ["\n".join(self.summary_lines())]
        blocks += [result.render() for result in self.results]
        return "\n" + "\n".join(blocks)

    def to_json(self) -> str:
        return json.dumps(
            {
                "answer": self.headline,
                "trustworthy": not self.problems,
                "trust": self.trust,
                "rounds": self.rounds,
                "worst_drift_pct": round(self.worst_drift, 2),
                "cpu_saving_per_gb": (
                    round(self.cpu.saving, 5) if self.cpu.saving is not None else None
                ),
                "comparisons": [
                    c.to_dict() for r in self.results for c in r.comparisons
                ],
            },
            indent=2,
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="compare two builds")
    parser.add_argument("records", nargs="?", default="/results/records.jsonl")
    parser.add_argument("--json", dest="json_out")
    args = parser.parse_args()

    report = Report.load(args.records)
    print(report.render())
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(report.to_json())
        print(f"wrote {args.json_out}")
