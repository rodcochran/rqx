import asyncio
import time
from concurrent.futures import ThreadPoolExecutor

import aiohttp
import httpr
import httpx
import rqx

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

# Connection-pool ceiling for every client under test. Set above the highest
# CONCURRENCY_LEVELS entry so the pool never becomes the bottleneck — we're
# measuring engine throughput, not default-pool sizing.
MAX_CONNECTIONS = 1500


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
    # httpr doesn't expose a connection-pool ceiling on the client constructor —
    # it inherits whatever reqwest's defaults are. Documented limitation;
    # comparison with the other three is best-effort at high concurrency.
    return httpr.AsyncClient()


async def bench_rqx(concurrency, duration):
    async with _rqx_client() as client:
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


async def bench_httpr(concurrency, duration):
    async with _httpr_client() as client:
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
    async with _httpx_client() as client:
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
    async with _aiohttp_session() as session:
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
    # httpr.AsyncClient is not native async — it dispatches sync Rust calls to
    # asyncio's default ThreadPoolExecutor. The default executor is sized
    # min(32, cpu+4), which would cap httpr at ~6 concurrent in-flight requests
    # on a 2-vCPU box. Bump it to MAX_CONNECTIONS so httpr can actually
    # exercise concurrency comparable to the native-async clients. rqx, httpx,
    # and aiohttp don't use this executor — only httpr is affected.
    loop = asyncio.get_running_loop()
    loop.set_default_executor(ThreadPoolExecutor(max_workers=MAX_CONNECTIONS))

    results = {}
    for concurrency in CONCURRENCY_LEVELS:
        for name, fn in [
            ("rqx", bench_rqx),
            ("httpr", bench_httpr),
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
