import asyncio
import time

import reqx

TARGET_URL = "http://localhost:8080/json"
ITERATIONS = 1000
RUNS = 5


async def bench_no_retry():
    """Baseline - no retry configured"""
    async with reqx.AsyncClient() as client:
        times = []
        for _ in range(RUNS):
            start = time.perf_counter()
            for _ in range(ITERATIONS):
                await client.get(TARGET_URL)
            elapsed = (time.perf_counter() - start) * 1000
            times.append(elapsed)
    return times


async def bench_with_retry():
    """Retry configured but never triggered (first attempt succeeds)"""
    transport = reqx.AsyncHTTPTransport(
        retries=reqx.Retry(
            total=3,
            backoff_factor=0.5,
            status_forcelist={500, 502, 503},
        )
    )
    async with reqx.AsyncClient(transport=transport) as client:
        times = []
        for _ in range(RUNS):
            start = time.perf_counter()
            for _ in range(ITERATIONS):
                await client.get(TARGET_URL)
            elapsed = (time.perf_counter() - start) * 1000
            times.append(elapsed)
    return times


def print_results(name, times):
    mean = sum(times) / len(times)
    std = (sum((t - mean) ** 2 for t in times) / len(times)) ** 0.5
    per_call = mean / ITERATIONS
    print(f"{name}:")
    print(f"  Mean: {mean:.1f} ms  Std: {std:.1f} ms")
    print(f"  Per call: {per_call:.4f} ms ({per_call * 1000:.1f} µs)")
    print()


async def main():
    print(f"Iterations: {ITERATIONS}, Runs: {RUNS}\n")

    no_retry = await bench_no_retry()
    print_results("No retry configured", no_retry)

    with_retry = await bench_with_retry()
    print_results("Retry configured (never triggered)", with_retry)

    no_retry_mean = sum(no_retry) / len(no_retry) / ITERATIONS * 1000  # µs
    with_retry_mean = sum(with_retry) / len(with_retry) / ITERATIONS * 1000  # µs
    overhead = with_retry_mean - no_retry_mean
    print(f"Retry overhead per request: {overhead:.1f} µs")
    print("Target: ≤ 100 µs")


if __name__ == "__main__":
    asyncio.run(main())
