"""Generate static JSON payloads for the large-payload bench.

Writes response-100kb.json (~100 KB) and response-1mb.json (~1 MB) into the
current directory. The payloads are lists of moderately nested records —
realistic API-response shape, not a single giant string. Parsers have to walk
real structure.

Run once:
    cd benchmarks/nginx && python generate_payloads.py
"""

import json
from pathlib import Path


def make_record(i: int) -> dict:
    """~100 bytes per record after JSON encoding."""
    return {
        "id": i,
        "sku": f"item_{i:08d}",
        "name": f"Benchmark Item {i}",
        "price_cents": (i * 37) % 99_999,
        "in_stock": (i % 3) != 0,
        "tags": ["alpha", "beta", "gamma"][i % 3 : (i % 3) + 2],
    }


def build_payload(target_bytes: int) -> bytes:
    """Build a JSON list whose encoded size is close to target_bytes."""
    # Each record encodes to ~100 bytes; overshoot slightly, then trim.
    n = target_bytes // 100
    items = [make_record(i) for i in range(n)]
    encoded = json.dumps({"items": items}, separators=(",", ":")).encode()
    # If we're off by more than 10%, adjust and rebuild.
    while abs(len(encoded) - target_bytes) > target_bytes // 10:
        ratio = target_bytes / len(encoded)
        n = max(1, int(n * ratio))
        items = [make_record(i) for i in range(n)]
        encoded = json.dumps({"items": items}, separators=(",", ":")).encode()
    return encoded


def main() -> None:
    here = Path(__file__).parent
    # Docker Desktop's virtio networking on macOS can't sustain 100 KB or
    # larger payloads at realistic concurrency — runs against Docker cap at
    # ~10 KB. The larger sizes are used with the host-native nginx setup
    # (benchmarks/nginx/nginx-host.conf + `nginx -c`), which doesn't go
    # through the VM and handles the bandwidth without issue.
    for label, size in [("10kb", 10_000), ("100kb", 100_000), ("1mb", 1_000_000)]:
        payload = build_payload(size)
        path = here / f"response-{label}.json"
        path.write_bytes(payload)
        print(f"Wrote {path} — {len(payload):,} bytes ({len(payload) / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
