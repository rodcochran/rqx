import argparse
import asyncio
import json
import resource
import time
from concurrent.futures import ThreadPoolExecutor

import httpr

TARGET_URL = "http://localhost:8080/json"
MAX_CONNECTIONS = 1500


async def bench(concurrency, duration):
    # httpr.AsyncClient uses asyncio's default ThreadPoolExecutor to run sync
    # reqwest calls. We don't change its connection pool (no API for it); the
    # tradeoff is documented.
    async with httpr.AsyncClient() as client:
        count = 0

        async def worker():
            nonlocal count
            deadline = time.monotonic() + duration
            while time.monotonic() < deadline:
                try:
                    r = await client.get(TARGET_URL)
                    _ = r.content
                    count += 1
                except Exception:
                    pass

        await asyncio.gather(*[worker() for _ in range(concurrency)])
    return count


async def main():
    p = argparse.ArgumentParser()
    p.add_argument("--c", type=int, required=True)
    p.add_argument("--warmup", type=int, default=3)
    p.add_argument("--measure", type=int, default=15)
    p.add_argument("--run", type=int, default=1)
    args = p.parse_args()

    # httpr is sync-on-executor; the default executor on a 2-vCPU box is min(32,
    # cpu+4) ≈ 6 workers, which caps in-flight requests far below the test's
    # concurrency. Size to MAX_CONNECTIONS so the executor isn't the limit.
    loop = asyncio.get_running_loop()
    loop.set_default_executor(ThreadPoolExecutor(max_workers=MAX_CONNECTIONS))

    await bench(args.c, args.warmup)
    count = await bench(args.c, args.measure)

    peak_rss_kb = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    print(
        json.dumps(
            {
                "client": "httpr",
                "concurrency": args.c,
                "run": args.run,
                "rps": count / args.measure,
                "peak_rss_mb": peak_rss_kb / 1024,
                "measure_seconds": args.measure,
                "count": count,
            }
        )
    )


if __name__ == "__main__":
    asyncio.run(main())
