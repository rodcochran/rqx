import asyncio
import time

import aiohttp
import httpx
import numpy as np
import reqx

TARGET_URL = "http://localhost:8080/json"
CONCURRENCY = 100
TOTAL_REQUESTS = 10_000
REQUESTS_PER_WORKER = TOTAL_REQUESTS // CONCURRENCY  # 100 each


async def bench_reqx():
    latencies = []
    async with reqx.AsyncClient() as client:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                start = time.perf_counter()
                await client.get(TARGET_URL)
                elapsed = (time.perf_counter() - start) * 1000
                latencies.append(elapsed)

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])
    return latencies


async def bench_httpx():
    latencies = []
    async with httpx.AsyncClient() as client:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                start = time.perf_counter()
                await client.get(TARGET_URL)
                elapsed = (time.perf_counter() - start) * 1000
                latencies.append(elapsed)

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])
    return latencies


async def bench_aiohttp():
    latencies = []
    async with aiohttp.ClientSession() as session:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                start = time.perf_counter()
                async with session.get(TARGET_URL) as resp:
                    await resp.read()
                elapsed = (time.perf_counter() - start) * 1000
                latencies.append(elapsed)

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])
    return latencies


def print_percentiles(name, latencies):
    arr = np.array(latencies)
    print(f"\n{name} ({len(arr)} requests)")
    print(f"  p50:  {np.percentile(arr, 50):.2f} ms")
    print(f"  p75:  {np.percentile(arr, 75):.2f} ms")
    print(f"  p95:  {np.percentile(arr, 95):.2f} ms")
    print(f"  p99:  {np.percentile(arr, 99):.2f} ms")
    print(f"  p999: {np.percentile(arr, 99.9):.2f} ms")
    print(f"  max:  {np.max(arr):.2f} ms")


async def main():
    for name, fn in [
        ("reqx", bench_reqx),
        ("httpx", bench_httpx),
        ("aiohttp", bench_aiohttp),
    ]:
        print(f"\nRunning {name}...")
        latencies = await fn()
        print_percentiles(name, latencies)


if __name__ == "__main__":
    asyncio.run(main())
