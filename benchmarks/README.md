# Benchmarks

Everything under `benchmarks/` measures rqx against httpx, aiohttp and httpr on identical infrastructure. Two ways to run it:

- **Locally**, against nginx in Docker on your machine. Fast to set up, good for relative comparisons while you iterate. Absolute numbers are loopback numbers and don't transfer.
- **On AWS**, one command, about 95 minutes and 30 cents: a client and a server on separate instances in one availability zone. This is where release numbers come from. See [infra/README.md](infra/README.md).

Release reports and charts live in `benchmarks/<version>/`; the raw runs they were computed from are archived in `benchmarks/results/aws-<date>-v<version>/`.

## Getting around

`benchmarks/justfile` has a recipe for every step, local and AWS. From the repo root:

```bash
just benchmarks::            # list them
just benchmarks::local-up    # nginx in Docker on :8080
just benchmarks::b1 3        # throughput sweep, 3 runs
just benchmarks::b 8 --runs 3
just benchmarks::sweep       # the full local sweep
just benchmarks::compare b1_results.jsonl
```

## Local setup

All commands below run from the repository root.

```bash
uv sync                                          # dev tooling
uv pip install -e ".[benchmarks]" httpx aiohttp httpr
maturin develop --release                        # always release mode for numbers
python benchmarks/nginx/generate_payloads.py     # once: the 10 KB, 100 KB and 1 MB bodies
docker compose -f benchmarks/docker-compose.yaml up -d    # nginx on :8080
```

`just benchmarks::local-up` does the last two steps. The compose file serves three static JSON bodies from `benchmarks/nginx/`: `/json` (1.4 KB, committed), `/json/10kb` and `/json/100kb` (generated). Generate before the first `up`: if the mounted files don't exist, Docker creates empty directories in their place and the generator can no longer write them. `local-up` checks for both files and clears any such empty directories first.

Two benches need more than the compose stack:

- b7 needs the delay server: `python benchmarks/delay_server.py` (aiohttp on `:8081`, 100 ms per request).
- b9 above 10 KB and b10 need nginx running on the host rather than in Docker, with `benchmarks/nginx/nginx-host.conf` (`:8082` plain, `:8443` TLS with keep-alive off). Docker Desktop's virtual network caps out well below what a 100 KB × c=100 sweep pushes, and the VM crashes under a 1 MB one. b10 also needs a self-signed cert under `benchmarks/nginx/certs/`.

## The benches

| Bench | Question | Target | Run | Output |
|---|---|---|---|---|
| `b1_{rqx,httpr,httpx,aiohttp}.py` via `run_b1.sh` | Requests per second at c = 10, 50, 100, 500, 1000 | nginx `/json` | `bash benchmarks/run_b1.sh --runs 5 --out b1.jsonl` | one JSON line per (client, c, run) with `rps` and `peak_rss_mb`; summarize with `analyze_b1.py` |
| `b2_latency.py` | Per-request latency distribution at c=100, 10 000 requests | nginx `/json` | `python benchmarks/b2_latency.py` | p50/p75/p95/p99/p999/max per client |
| `b3_connection_pool.py` | How much connection reuse is worth, 1 000 sequential requests with and without a shared client | nginx `/json` | `python benchmarks/b3_connection_pool.py` | seconds and RPS per client, reuse speedup |
| `b4_memory.py` | Memory for 1 000 requests at c=100 | nginx `/json` | `python benchmarks/b4_memory.py` | tracemalloc peak and RSS before/after per client |
| `b5_json_parsing.py` | `response.json()` cost, 10 000 parses of a cached body | nginx `/json` | `python benchmarks/b5_json_parsing.py` | mean and per-call µs for rqx, httpx, stdlib |
| `b6_retry_overhead.py` | Cost of configuring `Retry` when it never fires, 10 000 requests | nginx `/json` | `python benchmarks/b6_retry_overhead.py` | mean, median, per-call µs, with and without retry |
| `b7_network_latency.py` | Throughput when each request takes 100 ms, c = 10 to 500. Separates concurrency models: a thread-pool client plateaus at workers ÷ 0.1 s | delay server `:8081` | `python benchmarks/b7_network_latency.py` | RPS per client per concurrency |
| `b8_concurrency_sweep.py` | Latency distribution as concurrency grows, c = 1, 10, 50, 100 | nginx `/json` | `python benchmarks/b8_concurrency_sweep.py --runs 3 --json b8.json` | percentiles and histogram per (client, c), per run and median |
| `b9_payload_sweep.py` | Does the ranking hold as bodies grow: 1.4 KB, 10 KB, 100 KB, 1 MB (host nginx only), at c = 100, 100, 30, 10 | nginx `/json*` | `python benchmarks/b9_payload_sweep.py --json b9.json` | RPS per client per payload |
| `b10_tls_handshake.py` | Handshake throughput with keep-alive off, c = 10 to 500 | host nginx `:8443` | `python benchmarks/b10_tls_handshake.py --json b10.json` | RPS per client per concurrency |

`b1` runs each (client, concurrency, run) in its own Python process: fresh event loop, fresh imports, fresh tokio runtime, no executor state left over from another client. That is why it has a driver script instead of a single file. It also skips aiohttp at c=1000, where its connector deadlocks under this harness, and tolerates a client crashing mid-sweep by recording a `skipped` row rather than losing the run.

b1, b7, b8, b9 and b10 are time-bounded, a few seconds of warmup then a fixed measurement window, so sample size scales with throughput. b2 through b6 use fixed request counts. b1 and b8 warm up on the client they measure, so the measured window starts with a warm pool.

`stream_ab/` is a separate harness for streaming changes: it builds two commits from source in a Linux container and compares them head to head (`just bench-stream`). It was written for the copy removed in #139 and caught the async chunking regression in #107. Its README explains the paired-comparison method.

## The full local sweep

```bash
bash benchmarks/run_all.sh
```

Rebuilds in release mode, starts the delay server, checks both targets, then runs b4, b7, b8 and b1 in that order (short to long, so a broken setup fails fast). Output goes to `/tmp/rqx_bench_<timestamp>/`.

Between benches it restarts nginx and sleeps ten seconds. Sustained high-concurrency HTTP over loopback leaves thousands of sockets in `TIME_WAIT`; a bench that starts in that state shows multi-second maximums that have nothing to do with the client. The restart plus the pause clears most of it. `run_b1.sh` sleeps five seconds between cells for the same reason.

## Reading results

```bash
python benchmarks/analyze_b1.py b1.jsonl        # median/min/max RPS and RSS per (client, c)
python benchmarks/plot_bench.py <results-dir> --out-dir benchmarks/<version>
```

`plot_bench.py` draws the throughput, memory and latency charts used in the README from a results directory containing `b1_results.jsonl` and the `b2_latency-run*.log` files.

Read the spread before the median. `analyze_b1.py` prints min and max next to the median for exactly this reason. A change smaller than the min-to-max spread of the unchanged clients on the same run is noise.

## Methodology

**Controls.** Every run measures all four clients on the same box in the same session. httpx, aiohttp and httpr never change between two rqx builds, so if they moved too, the box moved, not rqx. Read rqx's delta next to theirs before crediting anything to a code change.

**Rule out the toolchain.** `metadata.txt` in each AWS run records the rqx commit, Python and rustc versions, and the comparator versions. Diff `Cargo.lock` between the two refs as well; a dependency bump can move numbers on its own.

**Comparators are unpinned.** They're installed from PyPI at setup time. httpr changed how its async client dispatches work between 0.4 and 0.7, and its numbers moved accordingly; the version in `metadata.txt` is the only way to tell that apart from an rqx change.

**Same-machine A/B.** For a regression check, bench ref A then ref B on the same instances: `just benchmarks::up` once, then `just benchmarks::run A 3` and `just benchmarks::run B 3`. Three b1 runs is enough to see a real regression; five is the release setting.

**What the AWS numbers measure.** Same-VPC round trip is under a millisecond, so even the throughput and latency benches are client-CPU-bound. That's the point: it makes response-surface and dispatch changes visible. It also means throughput gaps shrink over a real network, where the wire dominates.

## Known confounders

- **Loopback on macOS.** Docker Desktop's virtual network is the bottleneck above about 10 KB payloads and it crashes under sustained 1 MB traffic. Use the host nginx config for those.
- **`TIME_WAIT` accumulation.** See the sweep section. Symptom: a p999 or max in the seconds while p99 is in the milliseconds.
- **OS scheduling.** On a laptop, anything else running (browser, indexer, another bench) moves single-digit percent. Even on an AWS instance running nothing else, the five b1 runs of the v0.1.5 session spread like this (max minus min over the median):

  | c | rqx | httpr | httpx | aiohttp |
  |---|---|---|---|---|
  | 10 | 1.3% | 1.4% | 3.7% | 15.5% |
  | 100 | 1.5% | 3.4% | 4.6% | 3.3% |
  | 500 | 6.3% | 3.5% | 27.3% | 1.4% |

  A delta inside that band is noise. httpx's wide spread at c=500 comes from its pool bookkeeping, see below.
- **httpx's pool bookkeeping.** b1, b2 and b8 set every client's pool limit to 1,500, so no client queues for a connection. httpx still falls behind as concurrency rises because httpcore 1.0 re-scans every pooled connection, polling each idle socket, whenever a request is queued or finishes; at c=100 that scan is most of its CPU. The measurement is real, but it reflects httpx serving hundreds of concurrent requests from one client, not httpx in general. The shared limit was checked locally: httpx's default limits (100 connections, 20 keep-alive) were no faster at c=10 or c=100 and slower at c=500, and a limit sized to the concurrency was about 40% faster at c=500 but still more than 20× behind rqx, so every client keeps the same 1,500 setting. Details in the [0.2.0 report](0.2.0/report.md#limitations).
- **aiohttp at c=1000** is skipped in b1 because its connector deadlocks under this harness. That's a harness interaction, not a verdict on aiohttp.
- **Ambient proxies.** Both httpx and rqx honor `HTTP_PROXY` and friends. Unset them before benching.

## Limitations

- A local run tells you whether a change made rqx faster or slower relative to the other clients on your machine. It does not tell you what any of them do in production.
- The AWS run adds a real network hop and separate client and server machines, but a single AZ is still the best case: no DNS, no TLS to a distant peer, no packet loss. Gaps measured here are upper bounds on what a user sees.
- Streaming memory is not benchmarked here; it's pinned by a test instead. hyper reads the socket only when the body is polled, so a slow consumer stalls the server through TCP flow control. The iterator holds at most one network chunk, plus `chunk_size` bytes when one is passed. `tests/unit/streaming/test_memory_bound.py` streams 128 MB through a deliberately slow consumer and asserts peak RSS growth stays under 64 MB (1 MB pieces cost about 30 MB of buffers and Python bytes in flight; unbounded reading would show as the whole body).
- Every bench here uses plaintext HTTP/1.1 with keep-alive except b10. TLS session cost and HTTP/2 multiplexing are not measured in the release numbers.
- Absolute numbers move a few percent between instance pairs on identical software. Compare within a run, and compare releases by their deltas against the controls, as the release reports do.
