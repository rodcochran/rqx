# reqx: The Rust-based drop-in replacement for httpx

## 1. Design Decisions

### Runtime Singleton

- **Shared Tokio Runtime**: Upon module import we initialize one shared instance of a Tokio runtime with a `OnceLock` to avoid per-client overhead This allows users to create clients without worry of the overhead of maintaining multiple Client objects in Python.

### Async bridging

- **Syncronous GIL release**: We leverage, `py.detach` and `RUNTIME.block_on()` to allow for a free-threaded runtime in Rust.
- **Futures**: The async client needs to return Python-awaitable futures so we leverage `pyo3-async-runtimes::tokio::future_into_py`.
- **Why two strategies?**: Sync needs to block the thread and async needs to return a Python awaitable.

### Retry Architecture

- **Retry on the transport layer**: By keeping the retry mechansim on the transport, we can keep the entire backoff-sleep-retry cycle in Rust.
- **Backoff with `tokio::time::sleep`**: By using sleep on the Tokio runtime, other threads can continue to run without FFI overhead.
- **Why Transport?**: Matches httpx's architecture and separates "what to send" from "how to deliver."

## 2. Performance Results

To evaluate performance, we set up 6 benchmarks:

- Throughput
- Latency
- Connection pooling
- Memory
- JSON parsing
- Retry overhead

Each of these were evaluated using an in-memory NGINX server and python scripts.

### Throughput

To measure throughput, we evaluated on two axes: concurrency and requests per second.

What the results show is that under highly concurrent load, reqx and aiohttp are nearly equal in throughput and httpx completely collapses. This is likely due to the overhead of materializing requests on the python event loop, whereas aiohttp offloads all of the work to C and reqx offloads to Rust.

| Client  | Concurrency | RPS        |
| ------- | ----------- | ---------- |
| aiohttp | 10          | **_6149_** |
| httpx   | 10          | 947        |
| reqx    | 10          | 5368       |
| aiohttp | 50          | 6687       |
| httpx   | 50          | 134        |
| reqx    | 50          | **_6923_** |
| aiohttp | 100         | **_7821_** |
| httpx   | 100         | 126        |
| reqx    | 100         | 6105       |
| aiohttp | 500         | 6429       |
| httpx   | 500         | 20         |
| reqx    | 500         | **_6287_** |
| aiohttp | 1000        | **_7317_** |
| httpx   | 1000        | 3          |
| reqx    | 1000        | 6387       |

### Latency

To measure latency we fired off 10000 requests for each client, where each evaluation run was configured with 100 concurrent workers that executed 100 sequential requests each.

The results show a similar pattern to the throughput results: reqx and aiohttp perform nearly identically, and httpx falls short. Notably, reqx had a 6.7x faster p50 than httpx though on-par with aiohttp.

| Percentile | reqx (ms)   | httpx (ms) | aiohttp (ms) |
| ---------- | ----------- | ---------- | ------------ |
| p50        | **_11.07_** | 74.73      | 12.13        |
| p75        | 21.00       | 81.22      | **_17.20_**  |
| p95        | 33.85       | 95.30      | **_28.89_**  |
| p99        | **_41.29_** | 100.25     | 45.71        |
| p999       | 56.45       | 114.02     | **_54.52_**  |
| max        | **_69.89_** | 120.26     | 70.44        |

### Connection Pooling

To measure the impact of connection pooling we ran 1000 sequential requests for each client, twice — once reusing a single client across all requests, and once instantiating a fresh client per request.

The results show a similar pattern to the latency and throughput results: reqx and aiohttp perform nearly identically, and httpx falls short. Notably, reqx had a 5.5x higher reused RPS than httpx though on-par with aiohttp, and benefited the most from pooling with a 6.0x speedup.

| Client  | With reuse (s) | With reuse (RPS) | No reuse (s) | No reuse (RPS) | Speedup |
| ------- | -------------- | ---------------- | ------------ | -------------- | ------- |
| reqx    | 0.48           | 2104             | 2.86         | 349            | 6.0×    |
| httpx   | 2.64           | 379              | 6.51         | 154            | 2.5×    |
| aiohttp | 0.36           | 2764             | 1.47         | 680            | 4.1×    |

### Memory

To measure memory we fired off 1000 requests for each client, where each evaluation run was configured with 100 concurrent workers that executed 10 sequential requests each. For each run we captured peak Python-traced allocation via `tracemalloc` alongside process RSS before and after.

The results show a similar pattern to the latency and throughput results: reqx and aiohttp stay lean, and httpx falls short. Notably, reqx's peak traced allocation was 89x smaller than httpx and 11x smaller than aiohttp.

| Client  | Traced current (KB) | Traced peak (KB) | RSS delta (MB) |
| ------- | ------------------- | ---------------- | -------------- |
| reqx    | **_70.1_**          | **_228.5_**      | 21.2           |
| httpx   | 19251.4             | 20413.8          | 38.9           |
| aiohttp | 505.8               | 2441.0           | **_0.2_**      |

> _Note: tracemalloc only captures Python-side allocations. reqx's actual memory footprint is higher than traced, as connection pools, HTTP parsers, and TLS buffers live on the Rust heap. The RSS delta (~20 MB for reqx vs ~39 MB for httpx) is a fairer comparison, though still imprecise due to shared process measurement. The directional finding holds: reqx's Python-side footprint is minimal by design._

### JSON Parsing

To measure JSON parsing we parsed a 1433-byte response payload 10000 times per run across 5 runs for each client, comparing reqx's `.json()` (which now delegates to Python's `json.loads` through a PyO3 call) to httpx's `.json()` (which calls `json.loads` natively) and stdlib `json.loads` directly as a baseline.

The results break from the pattern seen elsewhere: all three land within ~0.5 µs of each other, with reqx slightly trailing. Notably, reqx's 3.8 µs per call is ~15% slower than stdlib's 3.3 µs — since reqx is calling the same `json.loads` under the hood, the gap is pure FFI overhead with no compensating parsing speedup. This is a real drawback of the current implementation: callers who already hold the response bytes are better off invoking `json.loads` directly than routing through `resp.json()`.

| Parser                     | Mean (ms)  | Std (ms)  | Per call (µs) |
| -------------------------- | ---------- | --------- | ------------- |
| reqx (json.loads via pyo3) | 37.9       | 0.6       | 3.8           |
| httpx (json.loads)         | 36.4       | 0.3       | 3.6           |
| stdlib json.loads          | **_32.9_** | **_3.0_** | **_3.3_**     |

### Retry Overhead

To measure the overhead of configuring retries we fired off 10000 sequential requests per run across 10 runs (plus 2 warmup runs discarded) for reqx in two configurations: a baseline `AsyncClient` with no retry configured, and an `AsyncClient` with an `AsyncHTTPTransport` wrapping a `Retry(total=3, backoff_factor=0.5, status_forcelist={500, 502, 503})`. The retry path is never triggered since the target always returns 200 — we're measuring the cost of the wrapper itself, not the cost of an actual retry.

The results comfortably clear our 100 µs target: configuring retries adds ~27 µs per request at the median, well within budget. Notably, the retry wrapper's overhead is roughly a single-digit percent of the baseline per-request cost, which is what you'd expect from a wrapper whose fast path is one branch.

| Configuration                      | Per call median (µs) | Per call mean (µs) | CV    |
| ---------------------------------- | -------------------- | ------------------ | ----- |
| No retry configured                | **_952.6_**          | **_946.7_**        | 21.5% |
| Retry configured (never triggered) | 979.1                | 1000.9             | 15.9% |
| **Overhead**                       | **26.5**             | **54.3**           | —     |

> Run-to-run variance was high (CV ~20%) due to background activity on the benchmark host. The median overhead is the more reliable statistic here than the mean, since it's robust to outlier runs. Clean numbers from an idle machine are forthcoming; the directional finding (well under target) is not expected to change.

## 3. Correctness Story

Correctness of the API was challenging, not because of what to implement but on how to do it.

The most challenging components were things that have been solved before but applied in a new way.

- Async lifetimes
- Ownership accross the FFI boundary
- Redirect handling with owned reqeusts
- Streaming over FFI - holding connection open across **next** calls
- Cookie store variation between `reqwest` and `httpx`

## 4. Comparison to pyreqwest

`pyreqwest` offers a similar implementation in terms of adding a pyO3 port of reqwest.

The key differences lie in the contract and feature parity with httpx.

The goal of this project was to create a drop-in replacement for httpx as many frameworks are heavily reliant on httpx and would benefit greatly from a minimal API change that provides a massive performance upgrade, particularly in highly concurrent environments.

## 5. What Remains

This project was intended for learning. I wanted to dive deep into Rust, http, and creating something useful for python.

That being said, the v0 is rough. There's plenty to do and others should feel inclined to contribute.

Some open items at the time of writing the report:

- MockTransport to enable testing without hitting any actual network.
- Granular timeout configurations (on connect, read, etc.)
- Streaming on redirects
- Auth header cleansing
- A full correctness harness
- Deduplication of code
- Documentation
- Other optimizations
