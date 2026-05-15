"""Concurrency sweep — end-to-end latency distribution at varying concurrency.

Runs all four clients (rqx, httpr, httpx, aiohttp) at c = 1, 10, 50, 100 against
the configured nginx target. For each (client, concurrency) we capture the full
end-to-end latency distribution (percentiles + histogram) so we can see how the
p99/p50 ratio behaves.

Design notes:
- One client per library lives for the entire run — connection pool stays warm
  across concurrency levels.
- All clients are constructed with MAX_CONNECTIONS well above the highest
  concurrency tested, so the connection pool is never the bottleneck.
- Time-bounded sweeps (warmup + measure) instead of fixed request count. Keeps
  the sample size roughly proportional to throughput at each concurrency.
- Per-level warmup at the chosen concurrency (so connection pool scales to that
  level before we start the timer).
- 3 runs by default. Reports per-run numbers and the median. Run-to-run
  variance is visible.
- Failures are counted and printed, not silently swallowed.

Usage:
  python benchmarks/b8_concurrency_sweep.py
  python benchmarks/b8_concurrency_sweep.py --json out.json --runs 3
"""

import argparse
import asyncio
import json
import time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor

import aiohttp
import httpr
import httpx
import numpy as np
import rqx

TARGET_URL = "http://localhost:8080/json"
CONCURRENCY_LEVELS = [1, 10, 50, 100]
WARMUP_SECONDS = 2
MEASURE_SECONDS = 10
DEFAULT_RUNS = 3

# Connection-pool ceiling for every client. Above the highest concurrency
# tested so the pool isn't a confounder.
MAX_CONNECTIONS = 1500


# --------------------------------------------------------------------------
# Client factories — each returns an async context manager that yields the
# session-like client object. The get_fn knows how to make a request against
# that specific client type.
# --------------------------------------------------------------------------


def _rqx_client():
    transport = rqx.AsyncHTTPTransport(
        max_connections=MAX_CONNECTIONS,
        max_keepalive_connections=MAX_CONNECTIONS,
    )
    return rqx.AsyncClient(transport=transport)


def _httpx_client():
    return httpx.AsyncClient(
        limits=httpx.Limits(
            max_connections=MAX_CONNECTIONS,
            max_keepalive_connections=MAX_CONNECTIONS,
        ),
    )


def _aiohttp_session():
    connector = aiohttp.TCPConnector(
        limit=MAX_CONNECTIONS,
        limit_per_host=MAX_CONNECTIONS,
    )
    return aiohttp.ClientSession(connector=connector)


def _httpr_client():
    # httpr doesn't expose a pool-size knob on AsyncClient. Documented
    # limitation; comparison with the other three is best-effort.
    return httpr.AsyncClient()


async def rqx_get(client, url):
    return await client.get(url)


async def httpr_get(client, url):
    return await client.get(url)


async def httpx_get(client, url):
    return await client.get(url)


async def aiohttp_get(session, url):
    # aiohttp's response must be opened as an async context manager and
    # explicitly read so the connection returns to the pool.
    async with session.get(url) as resp:
        await resp.read()


# Order is fixed so runs are reproducible. We rely on warmup + multiple runs
# to dilute any residual order effects (TIME_WAIT pressure, JIT-style cache
# effects, etc).
CLIENTS = [
    ("rqx", _rqx_client, rqx_get),
    ("httpr", _httpr_client, httpr_get),
    ("httpx", _httpx_client, httpx_get),
    ("aiohttp", _aiohttp_session, aiohttp_get),
]
CLIENT_NAMES = [name for name, _, _ in CLIENTS]


async def sweep(client, get_fn, concurrency, duration):
    """Run a time-bounded sweep at the given concurrency.

    Returns (latencies_ms, failures).
    """
    latencies = []
    failures = 0
    deadline = time.monotonic() + duration

    async def worker():
        nonlocal failures
        while time.monotonic() < deadline:
            start = time.perf_counter()
            try:
                await get_fn(client, TARGET_URL)
            except Exception:
                failures += 1
                continue
            latencies.append((time.perf_counter() - start) * 1000)

    await asyncio.gather(*[worker() for _ in range(concurrency)])
    return latencies, failures


def summarize(latencies):
    if not latencies:
        return {
            "n": 0, "mean": float("nan"), "p50": float("nan"),
            "p75": float("nan"), "p95": float("nan"), "p99": float("nan"),
            "p999": float("nan"), "max": float("nan"), "std": float("nan"),
        }
    arr = np.array(latencies)
    return {
        "n": int(len(arr)),
        "mean": float(arr.mean()),
        "p50": float(np.percentile(arr, 50)),
        "p75": float(np.percentile(arr, 75)),
        "p95": float(np.percentile(arr, 95)),
        "p99": float(np.percentile(arr, 99)),
        "p999": float(np.percentile(arr, 99.9)),
        "max": float(arr.max()),
        "std": float(arr.std()),
    }


def median_stats(run_stats_list):
    """Given a list of stats dicts (one per run), return dict of medians."""
    if not run_stats_list:
        return {}
    keys = run_stats_list[0].keys()
    return {k: float(np.median([s[k] for s in run_stats_list])) for k in keys}


def ascii_histogram(latencies_ms, width=50, bins=20, clip_percentile=99.5):
    if not latencies_ms:
        return "  (no data)"
    arr = np.array(latencies_ms)
    upper = np.percentile(arr, clip_percentile)
    clipped = arr[arr <= upper]
    if len(clipped) == 0:
        return "  (no data)"
    counts, edges = np.histogram(clipped, bins=bins)
    max_count = counts.max() if counts.max() else 1
    lines = []
    for i in range(bins):
        lo, hi = edges[i], edges[i + 1]
        bar = "#" * int(width * counts[i] / max_count)
        lines.append(f"  {lo:6.2f}-{hi:6.2f} ms | {bar} {counts[i]}")
    lines.append(f"  (clipped at p{clip_percentile} = {upper:.2f} ms; max = {arr.max():.2f} ms)")
    return "\n".join(lines)


def print_sweep_result(client_name, concurrency, per_run_stats, pooled_latencies, failures_per_run):
    med = median_stats(per_run_stats)
    med["p99_over_p50"] = med["p99"] / med["p50"] if med["p50"] else float("nan")
    total_failures = sum(failures_per_run)

    print(f"\n{'=' * 72}")
    print(f"{client_name}  concurrency={concurrency}  runs={len(per_run_stats)}")
    print("=" * 72)
    print("  Per-run p50 / p99 / p99.9 / max (ms):")
    for i, s in enumerate(per_run_stats):
        print(
            f"    run {i+1}: n={s['n']:<6d} "
            f"p50={s['p50']:6.2f}  p99={s['p99']:6.2f}  "
            f"p99.9={s['p999']:6.2f}  max={s['max']:6.2f}  "
            f"failures={failures_per_run[i]}"
        )
    print(
        f"  Median:  n={int(med['n']):<6d} "
        f"mean={med['mean']:6.2f}ms  "
        f"p50={med['p50']:6.2f}  "
        f"p95={med['p95']:6.2f}  "
        f"p99={med['p99']:6.2f}  "
        f"p99.9={med['p999']:6.2f}  "
        f"max={med['max']:6.2f}  "
        f"p99/p50={med['p99_over_p50']:4.1f}x"
    )
    if total_failures:
        print(f"  ⚠  Total failures across runs: {total_failures}")
    print("  Pooled histogram:")
    print(ascii_histogram(pooled_latencies))
    return med


async def main(json_path=None, runs=DEFAULT_RUNS):
    # httpr.AsyncClient runs sync Rust on asyncio's default ThreadPoolExecutor.
    # Default size is min(32, cpu+4) — would cap httpr at ~6 in-flight on a
    # 2-vCPU box, badly skewing the high-concurrency comparison. Bump to
    # MAX_CONNECTIONS. Native-async clients (rqx, httpx, aiohttp) aren't
    # affected.
    loop = asyncio.get_running_loop()
    loop.set_default_executor(ThreadPoolExecutor(max_workers=MAX_CONNECTIONS))

    per_run_stats = defaultdict(list)
    pooled_latencies = defaultdict(list)
    failures_per_run = defaultdict(list)

    for run_idx in range(runs):
        print(f"\n\n########  RUN {run_idx + 1} / {runs}  ########")
        for name, factory, get_fn in CLIENTS:
            async with factory() as client:
                for concurrency in CONCURRENCY_LEVELS:
                    print(f"[{name} c={concurrency}] warmup...", flush=True)
                    await sweep(client, get_fn, concurrency, WARMUP_SECONDS)
                    print(f"[{name} c={concurrency}] measuring {MEASURE_SECONDS}s...", flush=True)
                    latencies, failures = await sweep(client, get_fn, concurrency, MEASURE_SECONDS)
                    stats = summarize(latencies)
                    key = (name, concurrency)
                    per_run_stats[key].append(stats)
                    pooled_latencies[key].extend(latencies)
                    failures_per_run[key].append(failures)
                    print(
                        f"[{name} c={concurrency}] run {run_idx+1}: "
                        f"n={stats['n']} p50={stats['p50']:.2f}ms "
                        f"p99={stats['p99']:.2f}ms max={stats['max']:.2f}ms "
                        f"failures={failures}"
                    )

    print("\n\n" + "=" * 72)
    print("FINAL RESULTS (median across runs)")
    print("=" * 72)

    final = {}
    for concurrency in CONCURRENCY_LEVELS:
        for name in CLIENT_NAMES:
            key = (name, concurrency)
            med = print_sweep_result(
                name, concurrency,
                per_run_stats[key],
                pooled_latencies[key],
                failures_per_run[key],
            )
            final[f"{name}_c{concurrency}"] = {
                "median_stats": med,
                "per_run_stats": per_run_stats[key],
                "failures_per_run": failures_per_run[key],
            }

    # p99/p50 ratio table
    print("\n" + "=" * 72)
    print("p99/p50 ratio vs concurrency (median across runs)")
    print("=" * 72)
    print(f"  {'client':<10s}", end="")
    for c in CONCURRENCY_LEVELS:
        print(f"  c={c:<4d}", end="")
    print()
    for name in CLIENT_NAMES:
        print(f"  {name:<10s}", end="")
        for c in CONCURRENCY_LEVELS:
            r = final[f"{name}_c{c}"]["median_stats"]["p99_over_p50"]
            print(f"  {r:5.1f}x", end="")
        print()

    # Variance check across runs
    print("\n" + "=" * 72)
    print("VARIANCE CHECK: p99 across runs (sorted / spread)")
    print("=" * 72)
    print(f"  {'client':<10s} {'c':<5s} {'runs p99 (ms)':<30s} {'spread':<10s}")
    for name in CLIENT_NAMES:
        for c in CONCURRENCY_LEVELS:
            key = (name, c)
            p99s = sorted([s["p99"] for s in per_run_stats[key]])
            spread = (p99s[-1] - p99s[0]) / p99s[len(p99s) // 2] if p99s[len(p99s) // 2] else 0
            p99_str = " / ".join(f"{v:.2f}" for v in p99s)
            print(f"  {name:<10s} {c:<5d} {p99_str:<30s} {spread*100:5.1f}%")

    if json_path:
        serializable = {
            "config": {
                "target_url": TARGET_URL,
                "concurrency_levels": CONCURRENCY_LEVELS,
                "warmup_seconds": WARMUP_SECONDS,
                "measure_seconds": MEASURE_SECONDS,
                "runs": runs,
                "max_connections": MAX_CONNECTIONS,
                "clients": CLIENT_NAMES,
            },
            "results": final,
        }
        with open(json_path, "w") as f:
            json.dump(serializable, f, indent=2)
        print(f"\nResults written to {json_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", help="Write results to JSON file")
    parser.add_argument("--runs", type=int, default=DEFAULT_RUNS, help="Number of runs (default 3)")
    args = parser.parse_args()
    asyncio.run(main(json_path=args.json, runs=args.runs))
