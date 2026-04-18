import asyncio
import time

import aiohttp
import httpx
import reqx

TARGET_URL = "http://localhost:8080/json"
WARMUP_SECONDS = 5
MEASURE_SECONDS = 30
CONCURRENCY_LEVELS = [
    10,
    50,
    100,
    500,
    1000,
]


async def bench_reqx(concurrency, duration):
    async with reqx.AsyncClient() as client:
        count = 0

        async def worker():
            nonlocal count
            deadline = time.monotonic() + duration
            while time.monotonic() < deadline:
                try:
                    await client.get(TARGET_URL)
                    count += 1
                except Exception:
                    pass

        await asyncio.gather(*[worker() for _ in range(concurrency)])
    return count


async def bench_httpx(concurrency, duration):
    async with httpx.AsyncClient() as client:
        count = 0

        async def worker():
            nonlocal count
            deadline = time.monotonic() + duration
            while time.monotonic() < deadline:
                try:
                    await client.get(TARGET_URL)
                    count += 1
                except Exception:
                    pass

        await asyncio.gather(*[worker() for _ in range(concurrency)])
    return count


async def bench_aiohttp(concurrency, duration):
    async with aiohttp.ClientSession() as session:
        count = 0

        async def worker():
            nonlocal count
            deadline = time.monotonic() + duration
            while time.monotonic() < deadline:
                try:
                    async with session.get(TARGET_URL) as resp:
                        await resp.read()
                    count += 1
                except Exception:
                    pass

        await asyncio.gather(*[worker() for _ in range(concurrency)])
    return count


async def run_benchmark(name, bench_fn, concurrency, warmup, measure):
    # warmup phase
    await bench_fn(concurrency, warmup)
    # measurement phase
    count = await bench_fn(concurrency, measure)
    rps = count / measure
    return rps


async def main():
    results = {}
    for concurrency in CONCURRENCY_LEVELS:
        for name, fn in [
            ("reqx", bench_reqx),
            ("httpx", bench_httpx),
            ("aiohttp", bench_aiohttp),
        ]:
            rps = await run_benchmark(
                name, fn, concurrency, WARMUP_SECONDS, MEASURE_SECONDS
            )
            results[(name, concurrency)] = rps
            print(f"{name} @ {concurrency}: {rps:.0f} RPS")

    # print summary table
    print("\n" + "=" * 60)
    print(f"{'Client':<10} {'Concurrency':<15} {'RPS':<10}")
    print("-" * 60)
    for (name, conc), rps in sorted(results.items(), key=lambda x: (x[0][1], x[0][0])):
        print(f"{name:<10} {conc:<15} {rps:<10.0f}")


if __name__ == "__main__":
    asyncio.run(main())
