"""TLS handshake throughput, concurrency sweep.

Isolates the cost of repeated TLS handshakes by hitting nginx with
`keepalive_timeout 0` — every request opens a new TCP + TLS connection.
The hypothesis: under high concurrency with no connection reuse,
reqx's multi-threaded tokio runtime should distribute the CPU cost of
concurrent handshakes across worker threads, while httpr's single-
threaded runtime serializes them through one thread. If the hypothesis
is right, reqx should pull ahead of httpr at c >= ~100 or so.

Important caveat on TLS backends:
  reqx inherits reqwest's default TLS feature, which on macOS resolves
  to `native-tls` (Apple SecureTransport). httpr explicitly uses
  `rustls-tls`. So this bench compares apples-to-oranges on the TLS
  implementation as well as on the runtime architecture. Any gap
  therefore has two plausible causes. A follow-up bench with reqx
  rebuilt on `rustls-tls` would isolate them — not done here.

Prereq:
  - Host nginx running with nginx-host.conf (includes the HTTPS server
    block on :8443 with keepalive disabled).
  - Self-signed cert under benchmarks/nginx/certs/.

Usage:
  python benchmarks/b10_tls_handshake.py
  python benchmarks/b10_tls_handshake.py --json out.json
"""

import argparse
import asyncio
import json
import ssl
import time
from pathlib import Path

import aiohttp
import httpr
import httpx
import reqx

TARGET_URL = "https://localhost:8443/json"
CONCURRENCY_LEVELS = [10, 50, 100, 200, 500]
WARMUP_SECONDS = 3
MEASURE_SECONDS = 10


async def sweep(client, get_fn, concurrency, duration):
    count = 0
    failures = 0
    deadline = time.monotonic() + duration

    async def worker():
        nonlocal count, failures
        while time.monotonic() < deadline:
            try:
                await get_fn(client, TARGET_URL)
            except Exception:
                failures += 1
                continue
            count += 1

    await asyncio.gather(*[worker() for _ in range(concurrency)])
    return count / duration, failures


async def reqx_get(client, url):
    return await client.get(url)


async def httpr_get(client, url):
    return await client.get(url)


async def httpx_get(client, url):
    r = await client.get(url)
    _ = r.content
    return r


async def aiohttp_get(session, url):
    async with session.get(url) as resp:
        await resp.read()


def make_reqx():
    return reqx.AsyncClient(transport=reqx.AsyncHTTPTransport(verify=False))


def make_httpr():
    return httpr.AsyncClient(verify=False)


def make_httpx():
    return httpx.AsyncClient(verify=False)


def make_aiohttp():
    # aiohttp uses a custom SSL context. Create one that skips verification
    # for the self-signed cert.
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    return aiohttp.ClientSession(connector=aiohttp.TCPConnector(ssl=ctx))


CLIENTS = [
    ("reqx",    make_reqx,    reqx_get),
    ("httpr",   make_httpr,   httpr_get),
    ("httpx",   make_httpx,   httpx_get),
    ("aiohttp", make_aiohttp, aiohttp_get),
]


async def run_client(name, make_client, get_fn, results):
    async with make_client() as client:
        for c in CONCURRENCY_LEVELS:
            print(f"  [{name:>7s}] c={c:>4d}  warmup…", flush=True)
            await sweep(client, get_fn, c, WARMUP_SECONDS)
            print(f"  [{name:>7s}] c={c:>4d}  measuring {MEASURE_SECONDS}s…", flush=True)
            rps, failures = await sweep(client, get_fn, c, MEASURE_SECONDS)
            results[(name, c)] = {"rps": rps, "failures": failures}
            print(f"  [{name:>7s}] c={c:>4d}  rps={rps:7.1f}  failures={failures}")


async def main(json_path=None):
    results = {}
    for idx, (name, make_client, get_fn) in enumerate(CLIENTS):
        print(f"\n=== client: {name} ===")
        try:
            await run_client(name, make_client, get_fn, results)
        except Exception as e:
            print(f"  client {name} failed: {e}")

    # summary
    print()
    print("=" * 72)
    print("RPS by concurrency (rows = concurrency, columns = client)")
    print("=" * 72)
    header = f"{'concur':<8}" + "".join(f"{n:>10}" for n, _, _ in CLIENTS)
    print(header)
    for c in CONCURRENCY_LEVELS:
        row = f"c={c:<6d}"
        for name, _, _ in CLIENTS:
            r = results.get((name, c))
            row += f"{(r['rps'] if r else float('nan')):>10.1f}"
        print(row)

    print()
    print("=" * 72)
    print("failures by concurrency")
    print("=" * 72)
    print(header)
    for c in CONCURRENCY_LEVELS:
        row = f"c={c:<6d}"
        for name, _, _ in CLIENTS:
            r = results.get((name, c))
            row += f"{(r['failures'] if r else 0):>10d}"
        print(row)

    if json_path:
        serializable = {
            "config": {
                "target_url": TARGET_URL,
                "concurrency_levels": CONCURRENCY_LEVELS,
                "warmup_seconds": WARMUP_SECONDS,
                "measure_seconds": MEASURE_SECONDS,
            },
            "results": {f"{n}|c{c}": v for (n, c), v in results.items()},
        }
        Path(json_path).write_text(json.dumps(serializable, indent=2))
        print(f"\nResults written to {json_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--json")
    args = parser.parse_args()
    asyncio.run(main(json_path=args.json))
