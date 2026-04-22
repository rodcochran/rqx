"""Throughput sweep across payload sizes.

Runs each client at a payload-appropriate concurrency against three sizes:
1.4 KB (default), 10 KB (mid-range API response), 100 KB (fat API response).
Answers the question: "does the relative ordering between clients hold as
payload size grows?"

A note on payload size ceiling:
  1 MB responses at c=100 would push ~1 GB/s through localhost. On macOS,
  Docker Desktop's virtio networking caps out well below that and the VM
  crashes under sustained load. We intentionally stop at 100 KB here. A real
  1-MB-payload bench needs nginx running on the host, not in Docker, which
  is a bigger setup change worth doing separately if/when the question is
  specifically "what does rqx do at file-download-sized responses?"

A note on concurrency per payload:
  Large payloads + high concurrency combine multiplicatively on Docker
  networking. At 100 KB × c=100, we saw rqx hit Docker's bandwidth ceiling
  and get ~5% request failures. Dropping to c=30 at 100 KB keeps total
  bytes-in-flight under ~3 MB and stays well below the ceiling. For the
  smaller payloads the Docker cap isn't binding so we keep c=100.

Methodology:
  - Per (client, payload): 3 s warmup + 10 s measurement.
  - Between payload sizes: restart nginx to drain TCP state, retry a
    couple of times if Docker is in a flaky state.
  - One run per combination — variance is payload-size-dominated, not
    run-to-run-dominated, so repeats are low ROI.
"""

import argparse
import asyncio
import json
import subprocess
import time
from pathlib import Path

import aiohttp
import httpr
import httpx
import rqx

DEFAULT_BASE_URL = "http://localhost:8080"

# (path, label, concurrency). URL is built from --base-url + path at runtime.
#
# Two regimes:
#   - Docker-backed nginx (base-url http://localhost:8080): safe up to 10 KB,
#     crashes at 100 KB+ because of the virtio networking ceiling on macOS.
#   - Host-native nginx (base-url http://localhost:8082, via nginx-host.conf):
#     no VM in the way, can handle 100 KB and 1 MB workloads.
#
# Concurrency is tuned per payload so total in-flight bytes stay sane on
# both backends.
PAYLOAD_SPECS = [
    ("1.4 KB", "/json",       100),
    ("10 KB",  "/json/10kb",  100),
    ("100 KB", "/json/100kb",  30),
    ("1 MB",   "/json/1mb",    10),
]
WARMUP_SECONDS = 3
MEASURE_SECONDS = 10
DRAIN_SECONDS = 5


async def sweep(client, get_fn, url, concurrency, duration):
    """Time-bounded throughput sweep. Returns (rps, total_bytes, failures)."""
    count = 0
    total_bytes = 0
    failures = 0
    deadline = time.monotonic() + duration

    async def worker():
        nonlocal count, total_bytes, failures
        while time.monotonic() < deadline:
            try:
                resp = await get_fn(client, url)
            except Exception:
                failures += 1
                continue
            try:
                body_len = len(resp.content) if hasattr(resp, "content") else 0
                total_bytes += body_len
            except Exception:
                pass
            count += 1

    await asyncio.gather(*[worker() for _ in range(concurrency)])
    return count / duration, total_bytes, failures


async def rqx_get(client, url):
    return await client.get(url)


async def httpr_get(client, url):
    return await client.get(url)


async def httpx_get(client, url):
    r = await client.get(url)
    _ = r.content
    return r


class _AiohttpResp:
    __slots__ = ("content",)
    def __init__(self, content):
        self.content = content


async def aiohttp_get(session, url):
    async with session.get(url) as resp:
        body = await resp.read()
    return _AiohttpResp(body)


def drain(base_url: str):
    """Pause between payload sizes to let TCP state on localhost relax.
    No nginx restart — host-native nginx doesn't need it and Docker-restart
    is flaky under load. A plain sleep is enough for TIME_WAIT drain on
    small batches."""
    for _ in range(30):
        try:
            subprocess.run(
                ["curl", "-sf", "-o", "/dev/null", f"{base_url}/json"],
                check=True,
                capture_output=True,
                timeout=1,
            )
            break
        except Exception:
            time.sleep(0.5)
    time.sleep(DRAIN_SECONDS)


async def run_client(name, cls, get_fn, base_url, payloads, per_payload_results):
    async with cls() as client:
        for i, (label, path, concurrency) in enumerate(payloads):
            url = base_url + path
            print(f"  [{name:>7s}] {label:>6s} (c={concurrency:>3d})  warmup…", flush=True)
            await sweep(client, get_fn, url, concurrency, WARMUP_SECONDS)
            print(f"  [{name:>7s}] {label:>6s} (c={concurrency:>3d})  measuring {MEASURE_SECONDS}s…", flush=True)
            rps, bytes_, failures = await sweep(client, get_fn, url, concurrency, MEASURE_SECONDS)
            mb_s = (bytes_ / (1024 * 1024)) / MEASURE_SECONDS
            per_payload_results[(name, label)] = {
                "concurrency": concurrency,
                "rps": rps,
                "mb_per_s": mb_s,
                "failures": failures,
            }
            print(
                f"  [{name:>7s}] {label:>6s} (c={concurrency:>3d})  "
                f"rps={rps:7.1f}  throughput={mb_s:6.1f} MB/s  failures={failures}"
            )
            if i != len(payloads) - 1:
                print("  drain…", flush=True)
                drain(base_url)


async def main(json_path=None, base_url=DEFAULT_BASE_URL, payload_filter=None):
    # Filter which payloads to run — useful for partial sweeps against one
    # backend. Defaults to "all four".
    if payload_filter is None:
        payloads = PAYLOAD_SPECS
    else:
        keep = set(payload_filter)
        payloads = [p for p in PAYLOAD_SPECS if p[0] in keep]

    results = {}

    clients = [
        ("rqx", rqx.AsyncClient, rqx_get),
        ("httpr", httpr.AsyncClient, httpr_get),
        ("httpx", httpx.AsyncClient, httpx_get),
        ("aiohttp", aiohttp.ClientSession, aiohttp_get),
    ]

    print(f"target: {base_url}")
    print(f"payloads: {[p[0] for p in payloads]}")

    for idx, (name, cls, get_fn) in enumerate(clients):
        print(f"\n=== client: {name} ===")
        if idx > 0:
            print("drain…")
            drain(base_url)
        try:
            await run_client(name, cls, get_fn, base_url, payloads, results)
        except Exception as e:
            print(f"  client {name} failed: {e}")

    # summary table
    print()
    print("=" * 72)
    print(f"{'client':<10} {'payload':<10} {'c':>5} {'RPS':>10} {'MB/s':>10} {'fail':>6}")
    print("-" * 72)
    for name, _, _ in clients:
        for label, _path, _c in payloads:
            r = results.get((name, label))
            if r is None:
                print(f"{name:<10} {label:<10}  —")
                continue
            print(
                f"{name:<10} {label:<10} {r['concurrency']:>5d} "
                f"{r['rps']:>10.1f} {r['mb_per_s']:>10.1f} {r['failures']:>6d}"
            )
        print()

    print("=" * 72)
    print("RPS by payload size (rows = payload, columns = client)")
    print("=" * 72)
    header = f"{'payload':<10}" + "".join(f"{n:>10}" for n, _, _ in clients)
    print(header)
    for label, _, _ in payloads:
        row = f"{label:<10}"
        for name, _, _ in clients:
            r = results.get((name, label))
            row += f"{(r['rps'] if r else float('nan')):>10.1f}"
        print(row)

    print()
    print("=" * 72)
    print("Throughput (MB/s) by payload size")
    print("=" * 72)
    print(header)
    for label, _, _ in payloads:
        row = f"{label:<10}"
        for name, _, _ in clients:
            r = results.get((name, label))
            row += f"{(r['mb_per_s'] if r else float('nan')):>10.1f}"
        print(row)

    if json_path:
        serializable = {
            "config": {
                "base_url": base_url,
                "warmup_seconds": WARMUP_SECONDS,
                "measure_seconds": MEASURE_SECONDS,
                "payloads": [
                    {"label": l, "path": p, "concurrency": c}
                    for l, p, c in payloads
                ],
            },
            "results": {f"{n}|{l}": v for (n, l), v in results.items()},
        }
        Path(json_path).write_text(json.dumps(serializable, indent=2))
        print(f"\nResults written to {json_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", help="Write results to JSON file")
    parser.add_argument(
        "--base-url",
        default=DEFAULT_BASE_URL,
        help=f"Target nginx (default: {DEFAULT_BASE_URL}). Use http://localhost:8082 "
             f"for host-native nginx (nginx-host.conf).",
    )
    parser.add_argument(
        "--payloads",
        help="Comma-separated list of payload labels to run (e.g. '10 KB,100 KB,1 MB'). "
             "Defaults to all four.",
    )
    args = parser.parse_args()
    payload_filter = (
        [p.strip() for p in args.payloads.split(",")] if args.payloads else None
    )
    asyncio.run(main(
        json_path=args.json,
        base_url=args.base_url,
        payload_filter=payload_filter,
    ))
