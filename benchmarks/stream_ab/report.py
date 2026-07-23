"""The printed answer and the JSON beside it."""

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass

from comparison import Comparison, Measurements, Metric, MetricResults, Metrics
from records import RunRecord


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
