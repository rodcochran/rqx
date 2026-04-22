import asyncio
import statistics
import time

import rqx

TARGET_URL = "http://localhost:8080/json"
ITERATIONS = 10_000
RUNS = 10
WARMUP_RUNS = 2


async def _run_loop(client, iterations):
    for _ in range(iterations):
        await client.get(TARGET_URL)


async def bench_no_retry():
    """Baseline - no retry configured"""
    async with rqx.AsyncClient() as client:
        # Warmup - discarded
        for _ in range(WARMUP_RUNS):
            await _run_loop(client, ITERATIONS)

        times = []
        for _ in range(RUNS):
            start = time.perf_counter()
            await _run_loop(client, ITERATIONS)
            elapsed = (time.perf_counter() - start) * 1000
            times.append(elapsed)
    return times


async def bench_with_retry():
    """Retry configured but never triggered (first attempt succeeds)"""
    transport = rqx.AsyncHTTPTransport(
        retries=rqx.Retry(
            total=3,
            backoff_factor=0.5,
            status_forcelist={500, 502, 503},
        )
    )
    async with rqx.AsyncClient(transport=transport) as client:
        # Warmup - discarded
        for _ in range(WARMUP_RUNS):
            await _run_loop(client, ITERATIONS)

        times = []
        for _ in range(RUNS):
            start = time.perf_counter()
            await _run_loop(client, ITERATIONS)
            elapsed = (time.perf_counter() - start) * 1000
            times.append(elapsed)
    return times


def summarize(name, times):
    mean = statistics.mean(times)
    median = statistics.median(times)
    std = statistics.stdev(times) if len(times) > 1 else 0.0
    cv = (std / mean * 100) if mean else 0.0
    per_call_mean = mean / ITERATIONS * 1000  # µs
    per_call_median = median / ITERATIONS * 1000  # µs
    print(f"{name}:")
    print(f"  Mean:   {mean:.1f} ms  Std: {std:.1f} ms  CV: {cv:.1f}%")
    print(f"  Median: {median:.1f} ms")
    print(f"  Per call (mean):   {per_call_mean:.1f} µs")
    print(f"  Per call (median): {per_call_median:.1f} µs")
    print()
    return {"mean": per_call_mean, "median": per_call_median, "cv": cv}


async def main():
    print(
        f"Iterations: {ITERATIONS}, Runs: {RUNS} (+ {WARMUP_RUNS} warmup discarded)\n"
    )

    no_retry = await bench_no_retry()
    no_retry_stats = summarize("No retry configured", no_retry)

    with_retry = await bench_with_retry()
    with_retry_stats = summarize("Retry configured (never triggered)", with_retry)

    overhead_mean = with_retry_stats["mean"] - no_retry_stats["mean"]
    overhead_median = with_retry_stats["median"] - no_retry_stats["median"]

    print(f"Retry overhead per request (mean):   {overhead_mean:.1f} µs")
    print(f"Retry overhead per request (median): {overhead_median:.1f} µs")
    print("Target: ≤ 100 µs")

    # Flag if variance is still too high to trust the conclusion
    max_cv = max(no_retry_stats["cv"], with_retry_stats["cv"])
    if max_cv > 5.0:
        print(
            f"\n⚠️  CV is {max_cv:.1f}% — variance is high; overhead figure is not reliable."
            " Consider reducing background load or using a more stable target server."
        )


if __name__ == "__main__":
    asyncio.run(main())
