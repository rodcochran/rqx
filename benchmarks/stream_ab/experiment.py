"""Runs every config against both builds, then prints the answer.

Called by entrypoint.sh once the two virtualenvs exist. Each measurement is a
separate process so it uses that build's installed wheel.
"""

from __future__ import annotations

import argparse
import subprocess
from dataclasses import dataclass
from pathlib import Path

from configs import Configs, RunConfig
from report import Report

BUILDS = ["base", "head"]


@dataclass(frozen=True)
class Experiment:
    rounds: int
    configs: list[RunConfig]
    base_url: str
    venvs: Path
    records: Path

    def order_for(self, round_number: int) -> list[str]:
        """Alternate which build goes first. Always running base first would
        hand a systematic advantage to whichever went second — warm caches, a
        CPU already at full speed — and since builds are compared within a
        round, that advantage would land in the result."""
        return BUILDS if round_number % 2 else list(reversed(BUILDS))

    def measure(self, build: str, config: RunConfig, round_number: int) -> str:
        result = subprocess.run(
            [
                str(self.venvs / build / "bin" / "python"),
                str(Path(__file__).parent / "measurement.py"),
                "--build", build,
                "--config", config.name,
                "--round", str(round_number),
                "--base-url", self.base_url,
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout.strip()

    def run(self) -> None:
        lines = []
        for round_number in range(1, self.rounds + 1):
            for config in self.configs:
                for build in self.order_for(round_number):
                    print(
                        f"round {round_number}/{self.rounds} | {build} | "
                        f"{config.name}",
                        flush=True,
                    )
                    lines.append(self.measure(build, config, round_number))
        self.records.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="run the streaming benchmark")
    parser.add_argument("--rounds", type=int, default=10)
    parser.add_argument("--only", default="", help='one config, e.g. "async 1mb 8"')
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--venvs", default="/venvs")
    parser.add_argument("--out", default="/results")
    parser.add_argument("--base-ref", default="")
    parser.add_argument("--head-ref", default="")
    args = parser.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    slug = args.only.replace(" ", "-") if args.only else "sweep"

    experiment = Experiment(
        rounds=args.rounds,
        configs=Configs.select(args.only),
        base_url=args.base_url,
        venvs=Path(args.venvs),
        records=out / f"records-{slug}.jsonl",
    )
    experiment.run()

    report = Report.load(
        str(experiment.records), base_ref=args.base_ref, head_ref=args.head_ref
    )
    print(report.render())
    (out / f"answer-{slug}.txt").write_text(report.render())
    (out / f"answer-{slug}.json").write_text(report.to_json())
    print(f"saved to {out}/answer-{slug}.txt")
