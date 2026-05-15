"""Render the launch-report charts (throughput + memory + latency).

Style mirrors the ty / Pyrefly / Pyright / mypy chart: dark background, single
purple bar per client, value labeled to the right.

Throughput + memory come from b1.jsonl. Latency comes from a hardcoded summary
of b2_latency.py output (we don't have a stable JSON artifact for b2 yet — the
numbers are the median of the run preserved in
benchmarks/results/aws-run-20260515/client/manual/b2_latency.log).

Usage:
    python benchmarks/plot_launch.py path/to/b1.jsonl [--out-dir docs]
"""

import argparse
import json
import statistics
from collections import defaultdict
from pathlib import Path

import matplotlib.pyplot as plt

BG = "#0f0f0f"
BAR = "#14b8a6"
TEXT = "#f0fdfa"
SUBTLE = "#94a3b8"


def load(path):
    runs = defaultdict(list)
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            if "skipped" in r:
                continue
            runs[(r["client"], r["concurrency"])].append(r)
    return runs


def medians_at(runs, c, key):
    out = {}
    for (client, conc), samples in runs.items():
        if conc != c:
            continue
        out[client] = statistics.median(s[key] for s in samples)
    return out


def render_bar_chart(values, units, title, out_path, value_fmt=None):
    # Sort descending — longest bar on top, shortest on bottom. Consistent
    # across throughput (largest = best) and memory/latency (largest = worst);
    # the visual story is "this bar stands out."
    clients = sorted(values, key=lambda c: -values[c])
    nums = [values[c] for c in clients]

    fig, ax = plt.subplots(figsize=(11, 4.5), dpi=150)
    fig.patch.set_facecolor(BG)
    ax.set_facecolor(BG)

    bars = ax.barh(clients, nums, color=BAR, height=0.35)
    ax.invert_yaxis()

    pad = max(nums) * 0.02
    for bar, value in zip(bars, nums):
        if value_fmt is not None:
            label = value_fmt(value)
        elif units == "RPS":
            label = f"{value:,.0f} RPS"
        elif units == "MB":
            label = f"{value:.1f} MB"
        elif units == "ms":
            label = f"{value:.2f} ms"
        else:
            label = f"{value:.2f} {units}"
        ax.text(
            bar.get_width() + pad,
            bar.get_y() + bar.get_height() / 2,
            label,
            color=TEXT,
            fontweight="bold",
            fontsize=15,
            va="center",
        )

    ax.set_xlim(0, max(nums) * 1.3)
    ax.set_xticks([])
    for spine in ax.spines.values():
        spine.set_visible(False)
    plt.setp(ax.get_yticklabels(), color=TEXT, fontsize=16, fontweight="bold")
    ax.tick_params(axis="y", length=0, pad=10)

    fig.text(
        0.5, 0.04, title,
        color=SUBTLE, style="italic", ha="center", fontsize=12,
    )

    plt.subplots_adjust(left=0.18, right=0.95, top=0.95, bottom=0.18)
    plt.savefig(out_path, facecolor=fig.get_facecolor(), bbox_inches="tight")
    plt.close(fig)
    print(f"wrote {out_path}")


# Hardcoded from b2_latency.log (median of one run, c=100, 10k requests/client).
# If b2 ever emits JSON, swap to loading it programmatically.
B2_P50_MS = {
    "rqx": 7.00,
    "aiohttp": 7.85,
    "httpr": 14.32,
    "httpx": 121.80,
}


def main():
    p = argparse.ArgumentParser()
    p.add_argument("jsonl")
    p.add_argument("--out-dir", default="docs")
    args = p.parse_args()

    runs = load(args.jsonl)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    render_bar_chart(
        medians_at(runs, 100, "rps"),
        units="RPS",
        title="HTTP requests per second at concurrency=100 — median of 5 runs, AWS c7i.large client.",
        out_path=out_dir / "launch_throughput.png",
    )
    render_bar_chart(
        medians_at(runs, 100, "peak_rss_mb"),
        units="MB",
        title="Peak resident memory at concurrency=100 — median of 5 runs, AWS c7i.large client.",
        out_path=out_dir / "launch_memory.png",
    )
    render_bar_chart(
        B2_P50_MS,
        units="ms",
        title="Median per-request latency (p50) at concurrency=100 — 10,000 requests per client.",
        out_path=out_dir / "launch_latency.png",
    )


if __name__ == "__main__":
    main()
