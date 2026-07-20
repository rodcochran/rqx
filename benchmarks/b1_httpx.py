import argparse
import asyncio
import json
import resource
import time

import httpx

TARGET_URL = "http://localhost:8080/json"
MAX_CONNECTIONS = 1500


async def bench(concurrency, duration):
    async with httpx.AsyncClient(
        limits=httpx.Limits(
            max_connections=MAX_CONNECTIONS,
            max_keepalive_connections=MAX_CONNECTIONS,
        ),
    ) as client:
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

    await bench(args.c, args.warmup)
    count = await bench(args.c, args.measure)

    peak_rss_kb = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    print(
        json.dumps(
            {
                "client": "httpx",
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
