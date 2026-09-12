"""Median throughput per (client, concurrency) from a b1 results file, next to a baseline run.

Usage: python benchmarks/compare_b1.py <b1_results.jsonl> [--baseline <dir-or-jsonl>]
The baseline defaults to the newest benchmarks/results/aws-* archive.
"""

import argparse
import json
import statistics
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

CLIENTS = ("rqx", "httpr", "httpx", "aiohttp")
CONCURRENCIES = (10, 50, 100, 500, 1000)
ARCHIVES = Path(__file__).resolve().parent / "results"


@dataclass(frozen=True)
class Cell:
    client: str
    concurrency: int
    samples: list

    @property
    def median(self) -> float:
        return statistics.median(self.samples)


def load(path: Path) -> dict:
    """Rows keyed by (client, concurrency); tolerates a driver log with JSON lines mixed in."""
    if path.is_dir():
        path = path / "b1_results.jsonl"
    rows = defaultdict(list)
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if "client" in row and "rps" in row:
            rows[(row["client"], row["concurrency"])].append(row["rps"])
    return {
        key: Cell(client=key[0], concurrency=key[1], samples=v)
        for key, v in rows.items()
    }


def newest_archive() -> Path | None:
    archives = sorted(
        p for p in ARCHIVES.glob("aws-*") if (p / "b1_results.jsonl").exists()
    )
    return archives[-1] if archives else None


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("results", type=Path)
    parser.add_argument("--baseline", type=Path, default=newest_archive())
    args = parser.parse_args()

    current = load(args.results)
    baseline = load(args.baseline) if args.baseline else {}
    if not current:
        print("no b1 rows yet")
        return
    runs = [len(cell.samples) for cell in current.values()]
    label = args.baseline.name if args.baseline else "none"
    print(f"b1: {sum(runs)} rows, {min(runs)} complete run(s); baseline {label}")
    print(f"{'client':<8}{'c':>6}{'n':>3}{'rps':>10}{'baseline':>10}{'delta':>8}")
    for client in CLIENTS:
        for concurrency in CONCURRENCIES:
            cell = current.get((client, concurrency))
            if cell is None:
                continue
            ref = baseline.get((client, concurrency))
            delta = (
                f"{(cell.median - ref.median) / ref.median * 100:+.1f}%"
                if ref
                else "n/a"
            )
            ref_text = f"{ref.median:.0f}" if ref else "-"
            print(
                f"{client:<8}{concurrency:>6}{len(cell.samples):>3}{cell.median:>10.0f}{ref_text:>10}{delta:>8}"
            )


if __name__ == "__main__":
    main()
