"""Prints the answer from a benchmark run.

Usage:
    compare.py <raw records file> [--detail] [--json out.json]
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass

from report import Report
from summary import RunInfo


@dataclass(frozen=True)
class Cli:
    raw: str
    json_out: str | None
    detail: bool
    run: RunInfo

    @classmethod
    def from_argv(cls) -> Cli:
        parser = argparse.ArgumentParser(
            description="did PR #139 make streaming cheaper?"
        )
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
            run=RunInfo(
                base_ref=args.base_ref,
                head_ref=args.head_ref,
                rounds=args.rounds,
                toolchain=args.toolchain,
            ),
        )

    def run_report(self) -> None:
        report = Report.load(path=self.raw, run=self.run)
        print(report.render(detail=self.detail))
        if self.json_out:
            with open(self.json_out, "w") as handle:
                handle.write(report.to_json())
            print(f"wrote {self.json_out}")


if __name__ == "__main__":
    Cli.from_argv().run_report()
