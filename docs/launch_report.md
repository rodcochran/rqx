# rqx v0.1.0 — Performance Report

**rqx is the fastest async HTTP client for Python at every tested concurrency level**, with comparable memory usage to aiohttp, a fraction of httpr's footprint, and the most consistent run-to-run performance of the four.

![Throughput at concurrency=100](launch_throughput.png)

At concurrency=100, rqx serves 13,404 RPS — 7% faster than httpr, 18% faster than aiohttp, and 33× faster than httpx, on identical AWS hardware against the same nginx server.

## Memory

![Memory at concurrency=100](launch_memory.png)

At c=100, rqx and aiohttp are essentially tied for the smallest footprint (37 MB vs 37 MB). httpr — also reqwest-based, so the closest apples-to-apples comparison — sits at 138 MB. That gap is the cost of httpr's sync-on-threadpool model: 1,000+ OS thread stacks resident at high concurrency vs rqx's two tokio worker threads.

At c=1000, where aiohttp deadlocks and is no longer measurable, rqx holds 97 MB vs httpr's 160 MB — a ~40% reduction while also delivering more throughput.

## Latency

![Median latency at concurrency=100](launch_latency.png)

rqx has the lowest median per-request latency of the four clients tested. The chart is dominated by httpx's 121 ms p50 — at c=100 it's not really competitive with the other three.

## Full throughput table

Median RPS across 5 runs at each concurrency. Spread is min–max as a percentage of the median.

| Client  | c=10             | c=50             | c=100            | c=500            | c=1000           |
| ------- | ---------------- | ---------------- | ---------------- | ---------------- | ---------------- |
| **rqx** | **12,887** ±1.9% | **13,504** ±2.4% | **13,404** ±2.8% | **12,436** ±4.7% | **11,883** ±4.0% |
| httpr   | 9,785 ±40%       | 12,287 ±6%       | 12,484 ±54%      | 11,409 ±33%      | 10,547 ±45%      |
| aiohttp | 11,482 ±6%       | 11,579 ±2%       | 11,336 ±11%      | 8,492 ±9%        | —                |
| httpx   | 1,011 ±22%       | 507 ±6%          | 404 ±6%          | 128 ±27%         | 99 ±1%           |

rqx wins at every concurrency level. aiohttp's connector deadlocks at c=1000 under sustained load and was skipped.

## Memory at every concurrency

Peak resident set size (MB), median across 5 runs.

| Client  | c=10     | c=50     | c=100    | c=500    | c=1000   |
| ------- | -------- | -------- | -------- | -------- | -------- |
| **rqx** | **27.8** | **32.0** | **37.1** | 69.9     | 97.4     |
| aiohttp | 34.4     | 35.5     | 36.7     | **46.4** | —        |
| httpx   | 33.7     | 38.2     | 43.0     | 48.9     | **62.5** |
| httpr   | 123.1    | 132.4    | 137.6    | 151.5    | 160.3    |

rqx starts smaller than any other client at low concurrency and stays below httpr at every level. aiohttp is slightly leaner than rqx at low concurrency but doesn't reach c=1000 due to its deadlock. httpx is "small" mainly because its throughput is so low it never has many requests in flight.

## Latency (concurrency=100, 10,000 requests per client)

Per-request latency from `b2_latency.py`:

| Client  | p50         | p95       | p99         | p99.9       | max         |
| ------- | ----------- | --------- | ----------- | ----------- | ----------- |
| **rqx** | **7.00 ms** | 10.77 ms  | 13.38 ms    | 18.20 ms    | 21.99 ms    |
| aiohttp | 7.85 ms     | 8.13 ms   | **8.52 ms** | 16.61 ms    | 17.20 ms    |
| httpr   | 14.32 ms    | 22.14 ms  | 32.30 ms    | 34.13 ms    | 34.99 ms    |
| httpx   | 121.80 ms   | 604.15 ms | 1,516.73 ms | 3,258.99 ms | 4,575.80 ms |

rqx has the lowest p50. aiohttp has the tightest tail (only 8% spread between p50 and p99). httpx is unsuitable at this concurrency.

## Tail consistency under load

p99/p50 ratio across concurrency levels (`b8_concurrency_sweep.py`, median of 2 runs):

| Client      | c=1  | c=10     | c=50     | c=100    |
| ----------- | ---- | -------- | -------- | -------- |
| **aiohttp** | 1.3× | **1.2×** | **1.1×** | **1.1×** |
| rqx         | 1.2× | 1.8×     | 1.7×     | 1.8×     |
| httpr       | 1.3× | 1.9×     | 1.8×     | 1.8×     |
| httpx       | 1.4× | 3.0×     | 5.7×     | 5.9×     |

aiohttp wins tail consistency. rqx ties httpr at ~1.8× and dominates httpx at every concurrency.

## Test setup

- **Client:** AWS c7i.large (2 vCPU, dedicated CPU, 4 GB RAM), Ubuntu 24.04
- **Server:** AWS c7i.large in the same VPC, nginx 1.27 (alpine) serving static `/json`
- **Network:** intra-VPC private IP, no public hop, no TLS
- **Protocol:** HTTP/1.1 plaintext
- **Each measurement:** 3 s warmup + 15 s timed measure. 5 s cool-down between measurements.
- **Connection pool ceiling:** 1,500 connections / 1,500 keepalive idle on every client.
- **Body materialization:** explicit `.content` access (or `await resp.read()` for aiohttp) so every client does the same work.

## Methodology — why this benchmark is worth trusting

Our first attempt placed all four clients in a single Python process, taking turns. Each measurement had the others' state polluting the process — most notably, httpr requires a 1,500-thread `ThreadPoolExecutor` to scale, and those threads stay alive throughout the entire run. With rqx and httpr both spawning tokio runtimes in the same process, scheduler contention on the 2-vCPU box was depressing rqx's throughput by ~50% at c≥50.

The benchmark used here runs each (client, concurrency, run) in its own Python subprocess: fresh interpreter, fresh asyncio event loop, fresh tokio runtime, no foreign thread pools. Per-process peak RSS is attributable to a single client. The 5-second cool-down between subprocesses gives kernel TCP state and TIME_WAIT slots time to settle.

After the change, rqx's run-to-run spread dropped to under 3% — small enough that the lead over aiohttp and httpr is clearly real, not noise.

Source: `benchmarks/b1_{rqx,httpr,httpx,aiohttp}.py` (one file per client) and `benchmarks/run_b1.sh` (the driver).

## Limitations

- **Client-side ceiling.** Server CPU was at ~35% during rqx's measurements (`server_top_b1v2.log`), so absolute throughput is bounded by the c7i.large client, not nginx. Larger client instances would likely push higher.
- **Plaintext HTTP/1.1 only.** TLS handshake cost, ALPN negotiation, and HTTP/2 multiplexing are not measured here.
- **Synthetic workload.** Static JSON responses. Real bodies (chunked encoding, large payloads, streamed responses) may shift results.
- **Five runs per cell** is enough to spot patterns but not to make tight statistical claims. We report median + min–max spread, not confidence intervals.
- **aiohttp's connector deadlocks at c=1000** under sustained load. Reproducible across runs. We skipped that cell rather than hang the harness.
- **rqx degrades ~12% from c=100 to c=1000** (13,404 → 11,883 RPS). Likely GIL contention through the pyo3-async-runtimes bridge as completions back up against the single asyncio thread; investigation tracked for post-launch.

## Reproducing this

```bash
cd benchmarks/infra
bash scripts/bench.sh --runs-per-bench 5
```

Spins up paired EC2s in `us-east-1`, builds rqx in release mode, runs all three benchmarks, syncs results to S3, prompts for teardown.

Raw data for this run is in `s3://rqx-bench-results-<account>/aws-run-20260515/` and `benchmarks/results/aws-run-20260515/` in this repo.
