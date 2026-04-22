import asyncio
import time

import aiohttp
import httpr
import httpx
import rqx

TARGET_URL = "http://localhost:8080/json"
TOTAL_REQUESTS = 1000


async def bench_with_reuse(name, bench_fn):
    start = time.perf_counter()
    await bench_fn()
    elapsed = time.perf_counter() - start
    print(f"{name} (with reuse): {elapsed:.2f}s ({TOTAL_REQUESTS / elapsed:.0f} RPS)")
    return elapsed


async def bench_without_reuse(name, bench_fn):
    start = time.perf_counter()
    await bench_fn()
    elapsed = time.perf_counter() - start
    print(f"{name} (no reuse):   {elapsed:.2f}s ({TOTAL_REQUESTS / elapsed:.0f} RPS)")
    return elapsed


async def rqx_with_reuse():
    async with rqx.AsyncClient() as client:
        for _ in range(TOTAL_REQUESTS):
            await client.get(TARGET_URL)


async def rqx_without_reuse():
    for _ in range(TOTAL_REQUESTS):
        async with rqx.AsyncClient() as client:
            await client.get(TARGET_URL)


async def httpr_with_reuse():
    async with httpr.AsyncClient() as client:
        for _ in range(TOTAL_REQUESTS):
            await client.get(TARGET_URL)


async def httpr_without_reuse():
    for _ in range(TOTAL_REQUESTS):
        async with httpr.AsyncClient() as client:
            await client.get(TARGET_URL)


async def httpx_with_reuse():
    async with httpx.AsyncClient() as client:
        for _ in range(TOTAL_REQUESTS):
            await client.get(TARGET_URL)


async def httpx_without_reuse():
    for _ in range(TOTAL_REQUESTS):
        async with httpx.AsyncClient() as client:
            await client.get(TARGET_URL)


async def aiohttp_with_reuse():
    async with aiohttp.ClientSession() as session:
        for _ in range(TOTAL_REQUESTS):
            async with session.get(TARGET_URL) as resp:
                await resp.read()


async def aiohttp_without_reuse():
    for _ in range(TOTAL_REQUESTS):
        async with aiohttp.ClientSession() as session:
            async with session.get(TARGET_URL) as resp:
                await resp.read()


async def main():
    for name, with_fn, without_fn in [
        ("rqx", rqx_with_reuse, rqx_without_reuse),
        ("httpr", httpr_with_reuse, httpr_without_reuse),
        ("httpx", httpx_with_reuse, httpx_without_reuse),
        ("aiohttp", aiohttp_with_reuse, aiohttp_without_reuse),
    ]:
        print(f"\n--- {name} ---")
        t_reuse = await bench_with_reuse(name, with_fn)
        t_no_reuse = await bench_without_reuse(name, without_fn)
        speedup = t_no_reuse / t_reuse
        print(f"  Connection reuse speedup: {speedup:.1f}x")


if __name__ == "__main__":
    asyncio.run(main())
