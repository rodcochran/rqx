import asyncio
import time

import aiohttp
import httpr
import httpx
import numpy as np
import rqx

TARGET_URL = "http://localhost:8080/json"
CONCURRENCY = 100
TOTAL_REQUESTS = 10_000
REQUESTS_PER_WORKER = TOTAL_REQUESTS // CONCURRENCY  # 100 each

# Connection-pool ceiling for every client. Set above CONCURRENCY so the
# pool never becomes the bottleneck — we're measuring per-request latency,
# not pool-acquisition delay.
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
    try:
        return httpr.AsyncClient(max_connections=MAX_CONNECTIONS)
    except TypeError:
        print("[warning] httpr.AsyncClient(max_connections=...) not supported; using default")
        return httpr.AsyncClient()


async def bench_rqx():
    latencies = []
    async with _rqx_client() as client:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                start = time.perf_counter()
                try:
                    await client.get(TARGET_URL)
                except Exception:
                    continue
                elapsed = (time.perf_counter() - start) * 1000
                latencies.append(elapsed)

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])
    return latencies


async def bench_httpr():
    latencies = []
    async with _httpr_client() as client:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                start = time.perf_counter()
                try:
                    await client.get(TARGET_URL)
                except Exception:
                    continue
                elapsed = (time.perf_counter() - start) * 1000
                latencies.append(elapsed)

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])
    return latencies


async def bench_httpx():
    latencies = []
    async with _httpx_client() as client:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                start = time.perf_counter()
                try:
                    await client.get(TARGET_URL)
                except Exception:
                    continue
                elapsed = (time.perf_counter() - start) * 1000
                latencies.append(elapsed)

        await asyncio.gather(*[worker() for _ in range(CONCURRENCY)])
    return latencies


async def bench_aiohttp():
    latencies = []
    async with _aiohttp_session() as session:

        async def worker():
            for _ in range(REQUESTS_PER_WORKER):
                start = time.perf_counter()
                try:
                    async with session.get(TARGET_URL) as resp:
                        await resp.read()
                except Exception:
                    continue
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
        ("rqx", bench_rqx),
        ("httpr", bench_httpr),
        ("httpx", bench_httpx),
        ("aiohttp", bench_aiohttp),
    ]:
        print(f"\nRunning {name}...")
        latencies = await fn()
        print_percentiles(name, latencies)


if __name__ == "__main__":
    asyncio.run(main())
