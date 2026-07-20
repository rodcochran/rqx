"""Summarize a b1_results.jsonl produced by run_b1.sh.

Usage: python benchmarks/analyze_b1.py b1_results.jsonl
"""

import json
import statistics
import sys
from collections import defaultdict


def main(path):
    runs = defaultdict(list)  # (client, c) -> list of {rps, peak_rss_mb}
    skipped = defaultdict(int)

    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            key = (r["client"], r["concurrency"])
            if "skipped" in r:
                skipped[key] += 1
                continue
            runs[key].append(r)

    clients = sorted({k[0] for k in runs})
    concurrencies = sorted({k[1] for k in runs})

    print(
        f"{'client':<10} {'c':<6} {'n':<4} {'rps_median':<12} "
        f"{'rps_min':<10} {'rps_max':<10} {'rss_mb_med':<12} {'rss_mb_max':<10}"
    )
    print("-" * 80)
    for c in concurrencies:
        for client in clients:
            samples = runs[(client, c)]
            if not samples:
                if skipped[(client, c)]:
                    print(f"{client:<10} {c:<6} skipped ({skipped[(client, c)]} runs)")
                continue
            rpss = [s["rps"] for s in samples]
            rsss = [s["peak_rss_mb"] for s in samples]
            print(
                f"{client:<10} {c:<6} {len(samples):<4} "
                f"{statistics.median(rpss):<12.0f} {min(rpss):<10.0f} {max(rpss):<10.0f} "
                f"{statistics.median(rsss):<12.1f} {max(rsss):<10.1f}"
            )
        print()


if __name__ == "__main__":
    main(sys.argv[1])
