"""Render the bench summary charts (throughput + memory + latency).

Style mirrors the ty / Pyrefly / Pyright / mypy chart: dark background, single
purple bar per client, value labeled to the right.

Throughput + memory come from b1_results.jsonl. Latency comes from b2_latency
run logs (b2_latency-run*.log) in the same directory if present; otherwise
falls back to the hardcoded launch-run values.

Output filenames are bare (`throughput.png`, `memory.png`, `latency.png`) —
the destination directory namespaces them by release or context (e.g.
`benchmarks/0.1.2/throughput.png`). The launch-report originals at
`docs/launch_*.png` are historical artifacts and are not regenerated here.

Usage:
    # Preferred: point at a results directory.
    python benchmarks/plot_bench.py benchmarks/results/aws-20260518-v012/ \\
        --out-dir benchmarks/0.1.2

    # Back-compat: hand a b1.jsonl directly. Uses hardcoded b2 fallback.
    python benchmarks/plot_bench.py path/to/b1.jsonl [--out-dir out]
"""

import argparse
import json
import re
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


def render_bar_chart(values, units, title, out_path, value_fmt=None,
                     label_overrides=None, footnote=None):
    # Sort descending — longest bar on top, shortest on bottom. Consistent
    # across throughput (largest = best) and memory/latency (largest = worst);
    # the visual story is "this bar stands out."
    clients = sorted(values, key=lambda c: -values[c])
    nums = [values[c] for c in clients]
    label_overrides = label_overrides or {}
    display_labels = [label_overrides.get(c, c) for c in clients]

    fig, ax = plt.subplots(figsize=(11, 4.5), dpi=150)
    fig.patch.set_facecolor(BG)
    ax.set_facecolor(BG)

    bars = ax.barh(display_labels, nums, color=BAR, height=0.35)
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
    if footnote:
        fig.text(
            0.5, 0.005, footnote,
            color=SUBTLE, style="italic", ha="center", fontsize=10,
        )

    plt.subplots_adjust(left=0.18, right=0.95, top=0.95, bottom=0.18)
    plt.savefig(out_path, facecolor=fig.get_facecolor(), bbox_inches="tight")
    plt.close(fig)
    print(f"wrote {out_path}")


# Launch-run fallback (median of one run, c=100, 10k requests/client). Used
# only when no b2_latency-run*.log files are found alongside the b1 jsonl.
B2_P50_MS_FALLBACK = {
    "rqx": 7.00,
    "aiohttp": 7.85,
    "httpr": 14.32,
    "httpx": 121.80,
}


def load_b2_p50(log_dir: Path) -> dict:
    """Aggregate b2_latency-run*.log p50 values across runs into {client: median_p50_ms}.

    Each log block looks like:

        rqx (10000 requests)
          p50:  6.72 ms
          p75:  8.28 ms
          ...

    Returns empty dict if no logs found, so the caller can fall back."""
    block_pat = re.compile(
        r"(\w+) \(\d+ requests\)\n((?:  p\d+(?:99)?:\s+[\d.]+ ms\n|  max:\s+[\d.]+ ms\n)+)"
    )
    p50_pat = re.compile(r"\s+p50:\s+([\d.]+) ms")
    per_client = defaultdict(list)
    for path in sorted(log_dir.glob("b2_latency-run*.log")):
        content = path.read_text()
        for client, body in block_pat.findall(content):
            m = p50_pat.search(body)
            if m:
                per_client[client].append(float(m.group(1)))
    return {c: statistics.median(v) for c, v in per_client.items() if v}


def main():
    p = argparse.ArgumentParser()
    p.add_argument(
        "results",
        help="Either a results directory (containing b1_results.jsonl + b2_latency-run*.log) "
        "or a path to a b1 jsonl file (back-compat).",
    )
    p.add_argument("--out-dir", default="docs")
    args = p.parse_args()

    results_path = Path(args.results)
    if results_path.is_dir():
        b1_path = results_path / "b1_results.jsonl"
        if not b1_path.exists():
            raise SystemExit(f"no b1_results.jsonl in {results_path}")
        b2_p50 = load_b2_p50(results_path) or B2_P50_MS_FALLBACK
    else:
        b1_path = results_path
        b2_p50 = B2_P50_MS_FALLBACK

    runs = load(str(b1_path))
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    render_bar_chart(
        medians_at(runs, 100, "rps"),
        units="RPS",
        title="HTTP requests per second at concurrency=100 — median of 5 runs, AWS c7i.large client.",
        out_path=out_dir / "throughput.png",
        label_overrides={"httpx": "httpx*"},
        footnote="* httpx number is anomalously low on this hardware; cause undiagnosed. See launch_report.md.",
    )
    render_bar_chart(
        medians_at(runs, 100, "peak_rss_mb"),
        units="MB",
        title="Peak resident memory at concurrency=100 — median of 5 runs, AWS c7i.large client.",
        out_path=out_dir / "memory.png",
    )
    render_bar_chart(
        b2_p50,
        units="ms",
        title="Median per-request latency (p50) at concurrency=100 — 10,000 requests per client.",
        out_path=out_dir / "latency.png",
    )


if __name__ == "__main__":
    main()
