import asyncio
import resource
import tracemalloc

import aiohttp
import httpr
import httpx
import rqx

TARGET_URL = "http://localhost:8080/json"
TOTAL_REQUESTS = 1000
CONCURRENCY = 100
REQUESTS_PER_WORKER = TOTAL_REQUESTS // CONCURRENCY


def get_rss_mb():
    """Get current RSS in MB"""
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024 * 1024)


async def bench_rqx():
    async with rqx.AsyncClient() as client:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                resp = await client.get(TARGET_URL)
                resp.json()

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])


async def bench_httpr():
    async with httpr.AsyncClient() as client:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                resp = await client.get(TARGET_URL)
                resp.json()

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])


async def bench_httpx():
    async with httpx.AsyncClient() as client:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                resp = await client.get(TARGET_URL)
                resp.json()

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])


async def bench_aiohttp():
    async with aiohttp.ClientSession() as session:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                async with session.get(TARGET_URL) as resp:
                    await resp.json()

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])


async def measure(name, fn):
    tracemalloc.start()
    rss_before = get_rss_mb()

    await fn()

    current, peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    rss_after = get_rss_mb()

    print(f"\n{name}:")
    print(f"  Python traced current: {current / 1024:.1f} KB")
    print(f"  Python traced peak:    {peak / 1024:.1f} KB")
    print(f"  RSS before: {rss_before:.1f} MB")
    print(f"  RSS after:  {rss_after:.1f} MB")


async def main():
    for name, fn in [
        ("rqx", bench_rqx),
        ("httpr", bench_httpr),
        ("httpx", bench_httpx),
        ("aiohttp", bench_aiohttp),
    ]:
        await measure(name, fn)


if __name__ == "__main__":
    asyncio.run(main())
