import argparse
import asyncio
import json
import resource
import sys
import time

import rqx

TARGET_URL = "http://localhost:8080/json"
MAX_CONNECTIONS = 1500
# ru_maxrss is bytes on macOS and KiB on Linux.
MAXRSS_UNIT = 1 if sys.platform == "darwin" else 1024


async def run(client, concurrency, duration):
    """Requests completed by `concurrency` workers sharing `client` over `duration` seconds."""
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

    transport = rqx.AsyncHTTPTransport(
        max_connections=MAX_CONNECTIONS,
        max_keepalive_connections=MAX_CONNECTIONS,
    )
    async with rqx.AsyncClient(transport=transport) as client:
        # Warm up and measure on the same client, so the measured window starts with a warm pool.
        await run(client, args.c, args.warmup)
        count = await run(client, args.c, args.measure)

    peak_rss_mb = (
        resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * MAXRSS_UNIT / (1 << 20)
    )
    print(
        json.dumps(
            {
                "client": "rqx",
                "concurrency": args.c,
                "run": args.run,
                "rps": count / args.measure,
                "peak_rss_mb": peak_rss_mb,
                "measure_seconds": args.measure,
                "count": count,
            }
        )
    )


if __name__ == "__main__":
    asyncio.run(main())
