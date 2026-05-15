import asyncio
import time

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
    # NOTE: assuming httpr.AsyncClient accepts a max_connections kwarg. If
    # the constructor signature differs, edit this factory — every other
    # client honors MAX_CONNECTIONS through its own native config knob.
    try:
        return httpr.AsyncClient(max_connections=MAX_CONNECTIONS)
    except TypeError:
        # Fall back to default-config httpr so the bench still runs.
        # Flag it loudly so we know the comparison isn't pool-matched.
        print("[warning] httpr.AsyncClient(max_connections=...) not supported; using default")
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
