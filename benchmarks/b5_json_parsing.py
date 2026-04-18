import asyncio
import json
import time

import httpx
import reqx

TARGET_URL = "http://localhost:8080/json"
ITERATIONS = 10_000
RUNS = 5


async def get_response_body():
    """Fetch the response body once for reuse"""
    async with reqx.AsyncClient() as client:
        resp = await client.get(TARGET_URL)
        return resp.content, resp.text()


def bench_reqx_json(content):
    """Build a PyResponse-like object and parse JSON"""
    client = reqx.Client()
    resp = client.get(TARGET_URL)
    # Now repeatedly parse the cached body
    times = []
    for _ in range(RUNS):
        start = time.perf_counter()
        for _ in range(ITERATIONS):
            resp.json()
        elapsed = (time.perf_counter() - start) * 1000
        times.append(elapsed)
    return times


def bench_stdlib_json(text):
    """Parse with Python's json module"""
    times = []
    for _ in range(RUNS):
        start = time.perf_counter()
        for _ in range(ITERATIONS):
            json.loads(text)
        elapsed = (time.perf_counter() - start) * 1000
        times.append(elapsed)
    return times


def bench_httpx_json():
    """Parse via httpx response"""
    client = httpx.Client()
    resp = client.get(TARGET_URL)
    times = []
    for _ in range(RUNS):
        start = time.perf_counter()
        for _ in range(ITERATIONS):
            resp.json()
        elapsed = (time.perf_counter() - start) * 1000
        times.append(elapsed)
    client.close()
    return times


def print_results(name, times):
    mean = sum(times) / len(times)
    std = (sum((t - mean) ** 2 for t in times) / len(times)) ** 0.5
    per_call = mean / ITERATIONS
    print(f"{name}:")
    print(f"  Mean: {mean:.1f} ms  Std: {std:.1f} ms")
    print(f"  Per call: {per_call:.4f} ms ({per_call * 1000:.1f} µs)")
    print()


def main():
    content, text = asyncio.run(get_response_body())

    print(f"JSON payload size: {len(content)} bytes")
    print(f"Iterations: {ITERATIONS}, Runs: {RUNS}\n")

    print_results("reqx (serde_json → Python)", bench_reqx_json(content))
    print_results("httpx (json.loads)", bench_httpx_json())
    print_results("stdlib json.loads", bench_stdlib_json(text))


if __name__ == "__main__":
    main()
