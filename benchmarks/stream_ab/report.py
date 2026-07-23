"""The printed answer and the JSON beside it."""

from __future__ import annotations

import argparse
import json
import textwrap
from dataclasses import dataclass

from comparison import Comparison, Measurements, Metric, MetricResults, Metrics
from records import RunRecord


@dataclass(frozen=True)
class Report:
    results: list[MetricResults]
    base_ref: str = ""
    head_ref: str = ""

    WIDTH = 78

    @classmethod
    def load(cls, path: str, base_ref: str = "", head_ref: str = "") -> Report:
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
        return cls(results=results, base_ref=base_ref, head_ref=head_ref)

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

    @staticmethod
    def remainder(count: int, outcome: str) -> str:
        """The trailing sentence about configs that did not settle, or nothing
        at all when every one of them did."""
        if count == 0:
            return ""
        subject = "The remaining one" if count == 1 else f"The other {count}"
        return f" {subject} {outcome}"

    @staticmethod
    def short(ref: str) -> str:
        return ref[:7] if ref else "the earlier commit"

    @property
    def comparison_word(self) -> str:
        better, worse = self.cpu.improvements(), self.cpu.regressions()
        if better and worse:
            return "faster in some experiments and slower in others than"
        if worse:
            return "slower than"
        if better:
            return "faster than"
        return "no different from"

    @property
    def headline(self) -> str:
        return (
            f"code from commit {self.short(self.head_ref)} is "
            f"{self.comparison_word} code from commit {self.short(self.base_ref)}."
        )

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
        configs = len(self.cpu.comparisons)
        text = (
            f"This ran {configs} configurations for {self.rounds} rounds each, "
            "with the two builds swapping which one went first every round so "
            "neither gets the benefit of a warmed-up machine. The machine's own "
            f"speed drifted by up to {self.worst_drift:.0f}% over the run"
        )
        if self.problems:
            return text + ". Treat the result as unreliable: " + "; ".join(
                self.problems
            ) + "."
        return text + ", which is inside the 10% this harness tolerates."

    @property
    def cpu_text(self) -> str:
        cpu = self.cpu
        total = len(cpu.comparisons)
        settled = len(cpu.improvements())
        if cpu.saving is None:
            return (
                f"No experiment out of {total} showed a change in CPU time that "
                "stands apart from ordinary run-to-run variation."
            )
        return (
            f"{settled} of {total} experiments used less CPU per gigabyte "
            f"streamed, saving about {abs(cpu.saving):.3f} seconds for every "
            f"gigabyte ({cpu.per_mb:.0f} microseconds per megabyte). Because "
            "that saving is a fixed amount, it shows up as a larger percentage "
            "where the baseline is small."
            + self.remainder(
                total - settled,
                "moved too little to separate from ordinary run-to-run "
                "variation, which is what a fixed saving predicts.",
            )
        )

    @property
    def memory_text(self) -> str:
        memory = self.memory
        total = len(memory.comparisons)
        best = memory.best()
        if best is None:
            return (
                f"None of the {total} experiments showed a change in peak "
                "memory that stands apart from ordinary variation."
            )
        settled = len(memory.improvements())
        return (
            f"{settled} of {total} experiments used less peak memory, the "
            f"largest being {abs(best.change_pct):.0f}% less at {best.config}."
            + self.remainder(
                total - settled,
                "moved too little to tell apart from ordinary variation.",
            )
        )

    def paragraph(self, label: str, text: str) -> str:
        return textwrap.fill(
            f"{label}: {text}", width=self.WIDTH, subsequent_indent="    "
        )

    def summary_lines(self) -> list[str]:
        return [
            textwrap.fill(
                f"ANSWER  {self.headline}",
                width=self.WIDTH,
                subsequent_indent="        ",
            ),
            "",
            self.paragraph("CPU", self.cpu_text),
            "",
            self.paragraph("Memory", self.memory_text),
            "",
            self.paragraph("Trust", self.trust),
            "",
        ]

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
    parser.add_argument("--base-ref", default="")
    parser.add_argument("--head-ref", default="")
    args = parser.parse_args()

    report = Report.load(args.records, args.base_ref, args.head_ref)
    print(report.render())
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(report.to_json())
        print(f"wrote {args.json_out}")
