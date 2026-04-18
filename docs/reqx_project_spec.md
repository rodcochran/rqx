# CS 262A: Advanced Topics in Computer Systems
## Project Specification: **reqx** — A Rust-Backed Python HTTP Client

**Course**: CS 262A — Advanced Topics in Computer Systems  
**Semester**: Fall 2026  
**Team Size**: 2–3 students  
**Language Requirements**: Rust (core), Python (API layer), minimal shell scripting for CI  
**Final Deliverable Due**: Week 14, Friday 11:59 PM PT  
**Spec Version**: 2.0 (updated to address crate name collision and add retry specification)

---

## Overview

Python's `httpx` library is the de facto standard for modern async HTTP in Python. Its API design is excellent. Its performance is not. The connection pool is protected by an `asyncio.Lock`-guarded dictionary. The HTTP/1.1 and HTTP/2 parsers (`h11`, `h2`) are pure Python. TLS termination goes through the CPython `ssl` module wrapping OpenSSL. Under high concurrency, all three become bottlenecks in ways that are structural, not fixable by optimizing Python.

This project asks you to build **reqx**: a Python HTTP client library with an httpx-compatible API backed by a Rust core using `hyper`, `tokio`, and `rustls`. The goal is to prove — with measurements — that the architectural gap is real and quantifiable, and to ship a library that a Python developer can adopt without changing their application code beyond the import statement.

This is a systems project. Correctness comes first, performance comes second, and API ergonomics come third. A library that is fast but returns wrong status codes, drops response bodies, or panics under concurrent load will not pass.

---

## A Note on Naming and Prior Art

Before you begin, search both PyPI and crates.io for `reqx`. You will find that **the name `reqx` is already taken on crates.io** by an existing pure-Rust HTTP transport library (`reqx` v0.1.31, by lvillis) that targets SDK authors writing API clients in Rust. It wraps `hyper` directly, adds retry/backoff/idempotency, and has no Python bindings. It is entirely unrelated to this project in purpose and audience, but the name conflict has practical consequences for your build.

**What this means for your project:**

The Python package name on PyPI — what users `pip install` — remains `reqx`. This name appears unclaimed on PyPI and is the only name your users will ever see. However, the internal Rust extension module you publish to crates.io (if you choose to publish it at all) **cannot** be named `reqx`. Use `reqx-core` or `_reqx_ext` for the Cargo package name. In practice, PyO3 extension modules are typically named with a leading underscore (`_reqx`) and are not independently published to crates.io, so this constraint has minimal operational impact.

Additionally: the existing `reqx` Rust crate's retry and backoff design is mature and worth studying before you design your own. Do not copy its code, but its `RetryPolicy` / `RetryClassifier` trait split is a clean pattern. The spec below incorporates retry as a first-class feature, and you are expected to arrive at your own design.

**Your internal Cargo.toml package name must be `reqx-core`.** Your Python package name is `reqx`. Your PyO3 extension module is `_reqx`. These are three different names for three different artifact types and must not be confused.

---

## Motivation and Background

### Why this matters

The Python async ecosystem has a performance ceiling that is not generally understood. `asyncio`'s event loop is fast at I/O dispatch but the per-coroutine overhead of scheduling, resumption, and callback dispatch is measured in microseconds, not nanoseconds. At 10,000 concurrent requests, these microseconds become the dominant cost. More concretely:

- `httpx`'s connection pool acquires an `asyncio.Lock` on every borrow and release. Under contention, this serializes connection acquisition.
- `h11` parses HTTP/1.1 responses by walking Python bytearray objects character by character. A 10 KB response header block involves tens of thousands of Python bytecode operations.
- `h2`'s flow control window management involves Python integer arithmetic on every DATA frame.
- Python's `ssl` module blocks the event loop thread during certificate verification on new connections.
- `httpx` has no built-in retry. Every production caller wraps it in a manual retry loop — typically with `tenacity` — adding Python overhead on every failed attempt and duplicating backoff logic across codebases.

A Rust core built on `hyper` and `tokio` sidesteps the first four. A retry layer built in Rust sidesteps the fifth. Together, they represent the full structural gap between what Python HTTP clients are and what they could be.

### Prior art

- **pycurl**: wraps libcurl. Fast but has an unusable API (callback-based, no context managers, no async support).
- **aiohttp**: async-native Python HTTP. Faster than httpx in some benchmarks but still pure Python.
- **granian**: Rust ASGI server that moves the server-side HTTP path to Rust. Proves the model but is a server, not a client.
- **reqwest**: A widely-used Rust HTTP client. We use `hyper` directly rather than reqwest to give us finer control over the connection pool and retry integration, and to avoid a redundant dependency on top of hyper (which reqwest itself wraps).
- **reqx (Rust crate)**: Pure-Rust SDK transport library wrapping hyper. Not a Python library. Its retry and TLS backend design is worth studying.
- **pyo3**: The Rust/Python FFI framework. Required for this project.
- **tenacity**: The Python retry library most httpx users reach for today. Your retry implementation must be at least as expressive as tenacity's common usage patterns.

Nobody has built a hyper-backed Python HTTP client with an httpx-compatible API and first-class retry support. That is the gap you are filling.

---

## Project Phases

The project is divided into five phases with checkpoints. Each checkpoint is graded independently. A strong checkpoint 1 that never reaches checkpoint 5 is better than a broken checkpoint 5.

### Phase 1 — Foundation (Weeks 1–3)

Establish the Rust/Python build pipeline, the runtime singleton, and a minimal sync client capable of making a GET request and returning a response with status code, headers, and body.

**Deliverables:**
- Working `maturin` build configuration producing an installable wheel, with `Cargo.toml` package name `reqx-core`
- `PyClient` struct exposed to Python with at least `get(url) -> PyResponse`
- `PyResponse` with `.status_code`, `.headers`, `.text()`, `.content`, `.json()`
- Tokio runtime singleton initialized at module import
- `pytest` suite with at least 10 passing tests against a local `httpbin` instance

**Checkpoint 1 success criteria:**
```python
import reqx

client = reqx.Client()
resp = client.get("https://httpbin.org/get")
assert resp.status_code == 200
assert "Content-Type" in resp.headers
body = resp.json()
assert body["url"] == "https://httpbin.org/get"
```

This must work. It must not crash. It must release the GIL during I/O (`py.allow_threads()`). Verify GIL release by running two sync requests in parallel Python threads and confirming they execute concurrently.

---

### Phase 2 — Full Sync and Async Clients (Weeks 4–7)

Expand the client to cover the full request surface: all HTTP methods, request headers, request bodies, query parameters, authentication, redirects, and timeouts. Implement the async client via `pyo3-anyio`. Implement context manager protocols for both clients.

**Deliverables:**
- `reqx.Client` (sync) implementing `get`, `post`, `put`, `patch`, `delete`, `head`, `options`, `request`
- `reqx.AsyncClient` (async) implementing the same methods as Python awaitables
- Request builder supporting: `headers`, `params`, `json`, `data`, `content`, `timeout`, `follow_redirects`, `auth`
- `PyResponse` implementing: `.raise_for_status()`, `.url`, `.elapsed`, `.encoding`, `.iter_bytes()` (streaming)
- Exception hierarchy matching httpx semantics (see Exception Specification below)
- Context manager support: `with reqx.Client() as c:` and `async with reqx.AsyncClient() as c:`
- `pytest-asyncio` test suite with at least 50 passing tests

**Checkpoint 2 success criteria:**

The following must work without error:

```python
# Sync
with reqx.Client(timeout=10.0) as client:
    resp = client.post(
        "https://httpbin.org/post",
        json={"key": "value"},
        headers={"X-Custom": "header"},
    )
    assert resp.status_code == 200
    assert resp.json()["json"]["key"] == "value"

# Async
async with reqx.AsyncClient() as client:
    resp = await client.get(
        "https://httpbin.org/get",
        params={"foo": "bar"},
    )
    assert resp.status_code == 200
    assert resp.url.endswith("?foo=bar")

# Timeout enforcement
with reqx.Client() as client:
    with pytest.raises(reqx.TimeoutException):
        client.get("https://httpbin.org/delay/30", timeout=1.0)
```

The async client must be usable with both `asyncio` and `anyio`. Test both. The async client must also be usable inside an existing asyncio event loop without spawning a new one (the Tokio runtime is separate from the Python event loop).

---

### Phase 3 — Retry (Weeks 8–10)

Implement a first-class retry system integrated directly into the Rust transport layer. This is the most design-intensive phase. Read the Retry Specification section carefully before writing any code.

**Deliverables:**
- `reqx.Retry` configuration object (see Retry Specification)
- `reqx.HTTPTransport` and `reqx.AsyncHTTPTransport` accepting a `retry` parameter
- Retry logic executing entirely in Rust with no Python callback overhead per attempt
- Per-attempt and total timeout budgets tracked independently
- `resp.num_retries` attribute on all responses indicating how many retries were consumed
- Retry history accessible via `resp.retry_history` (list of `(status_code_or_exception, elapsed_ms)` tuples)
- At least 30 additional tests covering retry scenarios

**Checkpoint 3 success criteria:**

```python
transport = reqx.HTTPTransport(
    retries=reqx.Retry(
        total=3,
        backoff_factor=0.5,
        status_forcelist=[429, 500, 502, 503, 504],
        allowed_methods=["GET", "POST"],
        respect_retry_after_header=True,
    )
)

with reqx.Client(transport=transport) as client:
    # Server returns 503 twice then 200
    resp = client.get("http://localhost/flaky")
    assert resp.status_code == 200
    assert resp.num_retries == 2

    # Exhausted retries raise
    with pytest.raises(reqx.MaxRetriesExceeded):
        client.get("http://localhost/always-503")

    # Non-retryable status does not retry
    resp = client.get("http://localhost/always-404")
    assert resp.status_code == 404
    assert resp.num_retries == 0
```

---

### Phase 4 — Connection Pool, HTTP/2, and Advanced Features (Weeks 11–12)

Implement connection pool configuration, HTTP/2 support, cookie jar management, certificate verification controls, proxy support, and streaming.

**Deliverables:**
- Configurable connection pool: `max_connections`, `max_keepalive_connections`, `keepalive_expiry`
- HTTP/2 support, selectable at client construction via `http2=True`
- Cookie jar: persistent across requests within a client instance, accessible via `resp.cookies` and `client.cookies`
- `verify=False` for development environments (with a visible deprecation warning on use)
- Custom CA certificate injection via `verify="/path/to/ca-bundle.pem"`
- Proxy support: `proxies={"https": "http://proxy:8080"}`
- Streaming responses: `resp.iter_bytes(chunk_size=N)` returning a Python iterator (sync) or async iterator (async)
- `reqx.Client(transport=reqx.MockTransport(...))` for test injection (see Testing Specification below)
- At least 80 passing tests total, including HTTP/2 tests against an HTTP/2-capable server

**Checkpoint 4 success criteria:**

```python
# HTTP/2
with reqx.Client(http2=True) as client:
    resp = client.get("https://http2.pro/api/v1")
    assert resp.http_version == "HTTP/2"

# Streaming
with reqx.Client() as client:
    with client.stream("GET", "https://httpbin.org/stream/20") as resp:
        lines = list(resp.iter_lines())
        assert len(lines) == 20

# Retry + HTTP/2 together
transport = reqx.AsyncHTTPTransport(
    http2=True,
    retries=reqx.Retry(total=2, status_forcelist=[503]),
)
async with reqx.AsyncClient(transport=transport) as client:
    resp = await client.get("https://httpbin.org/status/200")
    assert resp.status_code == 200
```

Connection pool limits must be enforced. If `max_connections=5`, no more than 5 connections should be open simultaneously. Write a test that verifies this with a slow mock transport.

---

### Phase 5 — Performance Benchmarks and Correctness Harness (Weeks 13–14)

Implement the full benchmark suite and correctness harness. Produce the final report.

**Deliverables:**
- Benchmark suite (see Benchmark Specification below)
- Correctness harness (see Correctness Specification below)
- Final written report (see Report Requirements below)
- Clean, documented codebase ready for open-source release

---

## Retry Specification

Retry is a first-class citizen, not an afterthought. It must be designed as part of the transport layer, not bolted on top of the Python client. This section defines the required behavior in full.

### The `reqx.Retry` object

```python
reqx.Retry(
    total=3,                          # maximum total retry attempts (across all failure modes)
    connect=None,                     # max retries on connection errors (defaults to total)
    read=None,                        # max retries on read errors (defaults to total)
    status=None,                      # max retries on bad status codes (defaults to total)
    backoff_factor=0.0,               # multiplier for exponential backoff between retries
    backoff_max=120.0,                # ceiling on computed backoff delay in seconds
    backoff_jitter=0.0,               # random jitter added to backoff (0.0 = no jitter)
    status_forcelist=frozenset(),     # set of status codes that trigger a retry
    allowed_methods=frozenset([       # only retry requests with these HTTP methods
        "DELETE", "GET", "HEAD",
        "OPTIONS", "PUT", "TRACE"
    ]),
    respect_retry_after_header=True,  # honor Retry-After header delay when present
    raise_on_status=True,             # raise MaxRetriesExceeded when retries exhausted
    raise_on_redirect=True,           # raise TooManyRedirects when redirect loop detected
)
```

All parameters are keyword-only. `total=0` means no retries (single attempt). `total=None` means retry indefinitely — this is dangerous and must emit a `ReqxWarning` at construction time.

### Backoff calculation

The delay before attempt `n` (1-indexed, so `n=1` is the first retry) must be computed as:

```
delay = min(backoff_factor * (2 ** (n - 1)), backoff_max)
delay = delay + random.uniform(0, backoff_jitter)
```

The sleep happens in Rust (Tokio's `time::sleep`), not in Python. No `time.sleep()` calls in the Python layer. The delay must not count against `connect` or `read` timeout budgets — only against the total request budget if one is set.

### Retry-After header

When `respect_retry_after_header=True` and the response contains a `Retry-After` header:

- If the value is an integer, treat it as seconds to wait.
- If the value is an HTTP date string, compute the delta from now.
- The Retry-After delay **overrides** the computed backoff delay for that attempt.
- Cap the Retry-After delay at `backoff_max`. If the server requests a delay longer than `backoff_max`, use `backoff_max` and log a warning.
- Only honor `Retry-After` on `429` and `503` responses. Ignore it on all other status codes.

### What is retryable

A request attempt is eligible for retry if **all** of the following are true:

1. The request method is in `allowed_methods`
2. The retry budget for the failure type (`connect`, `read`, or `status`) is not exhausted
3. The total retry budget (`total`) is not exhausted
4. For status-based retries: the response status code is in `status_forcelist`
5. For connection/read errors: the error is classified as transient (see below)

**Transient errors eligible for retry:**
- `ConnectError` (TCP connection refused, DNS failure)
- `ConnectTimeout`
- `ReadTimeout`
- `ConnectionReset` (server closed the connection mid-response)

**Errors that must NOT be retried regardless of configuration:**
- `SSLError` / certificate verification failure (retrying will not fix a cert problem)
- `ProxyError`
- `TooManyRedirects`
- Any error on a request whose body has already been partially sent and cannot be rewound (i.e., streaming request bodies)

### Request body rewinding

For retries to work correctly on POST/PUT/PATCH, the request body must be rewindable. The behavior must be:

- `json=` and `data=` (dict) bodies: always rewindable (reconstructed from the original data structure in Rust)
- `content=bytes`: always rewindable (bytes buffer seeked back to 0)
- `content=iterator` or streaming body: **not rewindable** — attempting to retry a streaming body must raise `ReqxError("Streaming request bodies cannot be retried")` at the point the retry would occur, not at construction time

### Retry history

Every `PyResponse` must carry:

```python
resp.num_retries       # int: number of retry attempts consumed (0 if first attempt succeeded)
resp.retry_history     # list[RetryRecord]

# RetryRecord fields:
record.attempt         # int: 1-indexed attempt number that produced this record
record.status_code     # int | None: response status code, or None if a transport error
record.exception       # str | None: exception class name if a transport error, else None
record.elapsed_ms      # float: wall-clock time for this attempt in milliseconds
record.backoff_ms      # float: delay waited before the *next* attempt (0 for the last record)
```

When `total=0` (no retries), `resp.num_retries` is `0` and `resp.retry_history` is `[]`.

### `MaxRetriesExceeded`

When retry budget is exhausted, raise `reqx.MaxRetriesExceeded`. It must carry:

```python
except reqx.MaxRetriesExceeded as e:
    e.request          # the original PyRequest
    e.retry_history    # same structure as resp.retry_history
    e.last_response    # PyResponse | None: last response received (None if all attempts were transport errors)
    e.last_exception   # Exception | None: last transport exception if final attempt was a transport error
```

### Retry is transport-level, not client-level

Retry configuration lives on `reqx.HTTPTransport` / `reqx.AsyncHTTPTransport`, not on `reqx.Client`. This matches httpx's architecture and is not negotiable:

```python
# Correct
transport = reqx.HTTPTransport(retries=reqx.Retry(total=3))
client = reqx.Client(transport=transport)

# Also supported: shorthand on Client for common case
client = reqx.Client(retries=reqx.Retry(total=3))  # internally constructs HTTPTransport

# Wrong — this must raise TypeError with a helpful message
client = reqx.Client(retry=3)  # misspelling — catch and suggest correct spelling
```

---

## Functional Requirements

### FR-1: API Compatibility

The public API of `reqx.Client` and `reqx.AsyncClient` must be a strict subset of `httpx.Client` and `httpx.AsyncClient`. Every parameter accepted by `reqx` must behave identically to `httpx` for the same input. Parameters not yet implemented must raise `NotImplementedError` with a message indicating they are planned.

Specifically, the following must be behaviorally identical to httpx:

| Feature | Requirement |
|---|---|
| Status codes | Must match httpx exactly for all 1xx–5xx responses |
| Response headers | Must preserve case, order (insertion order), and duplicate headers as a multi-dict |
| Response body | Must be byte-identical to the actual response body |
| Redirect following | Must follow the same redirect chain as httpx with the same `follow_redirects` default (False) |
| Timeout semantics | `timeout` applies to the total request; `connect`, `read`, `write`, `pool` granularity is a stretch goal |
| `.raise_for_status()` | Must raise `reqx.HTTPStatusError` for 4xx and 5xx, identical semantics to httpx |
| Cookie handling | Cookies set by the server must be sent on subsequent requests to the same domain |
| Retry behavior | See Retry Specification above; no equivalent in httpx (this is an extension) |

### FR-2: Exception Hierarchy

The exception hierarchy must be importable and catchable. Existing code using `except httpx.TimeoutException` should work with `except reqx.TimeoutException` after a one-line import change.

```
reqx.ReqxError
├── reqx.RequestError
│   ├── reqx.TransportError
│   │   ├── reqx.TimeoutException
│   │   │   ├── reqx.ConnectTimeout
│   │   │   ├── reqx.ReadTimeout
│   │   │   ├── reqx.WriteTimeout
│   │   │   └── reqx.PoolTimeout
│   │   ├── reqx.NetworkError
│   │   │   ├── reqx.ConnectError
│   │   │   ├── reqx.ReadError
│   │   │   └── reqx.WriteError
│   │   └── reqx.ProxyError
│   └── reqx.HTTPStatusError          (raised by raise_for_status())
└── reqx.MaxRetriesExceeded           (raised when retry budget exhausted)
```

`reqx.ReqxWarning` (not an exception) must be emitted via Python's `warnings` module when `Retry(total=None)` is constructed.

All exceptions must carry `.request` (the `PyRequest` that caused the error) and, where applicable, `.response` (the `PyResponse` received before the error).

### FR-3: Thread and Coroutine Safety

- The sync client must be safe to use from multiple Python threads simultaneously.
- The async client must be safe to use from multiple concurrent coroutines. No Python-level locks should be held across `await` points.
- Destroying a client while requests are in-flight must not panic or corrupt memory. Document which behavior is implemented (cancel in-flight or let complete).
- The retry state machine must be safe under concurrent use — each in-flight request has its own independent retry state.

### FR-4: GIL Management

The Python GIL must be released during all network I/O **and during all retry backoff sleeps**. Specifically:
- `py.allow_threads()` must wrap every `runtime.block_on()` call in the sync path
- Tokio `time::sleep` inside the retry loop must execute without holding the GIL
- Verify with a threading test: two sync requests dispatched in parallel threads must execute in less than `max(t1, t2) * 1.1` wall-clock time

### FR-5: Memory Safety

- No `unsafe` Rust blocks except where required for PyO3 FFI (document each use)
- No memory leaks detectable by running the test suite under `valgrind --tool=memcheck`
- Response bodies must not be copied more than once between receipt from the network and delivery to Python. Document your zero-copy strategy.
- Retry history must not accumulate unbounded memory — cap stored history at `total + 1` entries.

---

## Non-Functional Requirements

### NFR-1: Performance Targets

These are minimum acceptable results measured on a c5.2xlarge (8 vCPU, 16 GB RAM) against a local nginx instance serving a 1 KB JSON response:

| Metric | reqx target | httpx baseline |
|---|---|---|
| Throughput (async, 1000 concurrent) | ≥ 3× httpx | measured |
| p50 latency (async, 100 concurrent) | ≤ 60% httpx p50 | measured |
| p99 latency (async, 100 concurrent) | ≤ 50% httpx p99 | measured |
| Memory per idle connection | ≤ 50% httpx | measured |
| Cold start import time | ≤ 200ms | measured |
| Retry overhead (per attempt, no sleep) | ≤ 100µs vs no-retry baseline | measured |

The retry overhead target isolates the cost of the retry state machine itself — the difference in latency on a successful first attempt between a client with `Retry(total=3)` configured and one with no retry configured must not exceed 100µs. Retry logic that costs more than this is leaking Python overhead into the hot path.

### NFR-2: Binary Size and Dependencies

- The compiled `.so` extension must be under 15 MB (stripped)
- The only required runtime dependencies for the Python package must be Python itself
- TLS must use `rustls`, not OpenSSL

### NFR-3: Python Version Support

- Must support Python 3.9–3.13 via `abi3-py39` stable ABI
- Wheels must be buildable via `maturin build --release` on Linux (x86_64, aarch64) and macOS (arm64)

### NFR-4: Error Messages

Rust panics must never propagate to Python as unhandled signals. All Rust errors must be caught and converted to the appropriate Python exception. A panic in Rust that kills the Python interpreter is an automatic deduction.

---

## Benchmark Specification

All benchmarks must be reproducible. Provide a `benchmarks/` directory with a `README.md` explaining how to run each benchmark from scratch.

### B-1: Throughput Benchmark

**Tool**: Custom asyncio harness (not wrk or vegeta — you need to measure client-side throughput)  
**Server**: Local nginx on localhost:8080 serving a fixed 1 KB JSON payload  
**Clients under test**: `reqx.AsyncClient`, `httpx.AsyncClient`, `aiohttp.ClientSession`  
**Concurrency levels**: 10, 50, 100, 500, 1000 concurrent coroutines  
**Measurement**: Requests per second (RPS), measured over a 30-second window after a 5-second warmup  
**Report**: A table and a line chart (RPS vs concurrency level) for all three clients

### B-2: Latency Distribution Benchmark

**Tool**: Custom asyncio harness measuring per-request latency with `time.perf_counter()`  
**Concurrency**: 100 concurrent coroutines, 10,000 total requests  
**Metrics**: p50, p75, p95, p99, p999, max latency in milliseconds  
**Report**: A table comparing reqx vs httpx, and a CDF plot of latency distributions

### B-3: Connection Pool Benchmark

**Scenario**: 1,000 requests to 100 different hostnames — measures connection establishment overhead, not steady-state throughput  
**Metric**: Total wall-clock time for all 1,000 requests  
**Goal**: Quantify the benefit of connection reuse

### B-4: Memory Benchmark

**Tool**: `tracemalloc` (Python) + `/proc/self/status` (Rust heap)  
**Scenario**: Create a client, make 1,000 requests, measure peak RSS and current RSS after GC  
**Report**: Memory per idle connection, memory per in-flight request, peak memory at 1,000 concurrent requests

### B-5: JSON Parsing Microbenchmark

**Scenario**: 10,000 calls to `resp.json()` on a cached 10 KB JSON response body (no network I/O)  
**Tool**: `timeit` with 5 runs, report mean and std dev

### B-6: Retry Overhead Microbenchmark

**Scenario**: 100,000 successful first-attempt requests through a `MockTransport`, comparing:
- Baseline: `reqx.Client()` with no retry configured
- With retry: `reqx.Client(retries=reqx.Retry(total=3, status_forcelist=[503]))`

**Metric**: Mean per-request latency delta between the two configurations  
**Goal**: Demonstrate retry overhead ≤ 100µs (NFR-1)  
**Report**: Distribution of per-request overhead across 100,000 samples

### B-7: Retry Throughput Benchmark

**Scenario**: Simulated flaky server via `MockTransport` returning 503 on the first attempt and 200 on the second. Measure total throughput at 100 concurrent coroutines making 10,000 total requests.  
**Comparison**: `reqx` with built-in retry vs `httpx` + `tenacity` decorator  
**Metric**: RPS and total wall-clock time  
**Goal**: Show that Rust-native retry is faster than Python-layer retry even accounting for the extra round-trip

---

## Correctness Specification

### C-1: HTTP RFC Compliance

```python
for code in [200, 201, 204, 301, 302, 400, 401, 403, 404, 500, 503]:
    resp = client.get(f"http://localhost/status/{code}")
    assert resp.status_code == code

resp = client.get("http://localhost/redirect/3")
assert resp.status_code == 302
resp = client.get("http://localhost/redirect/3", follow_redirects=True)
assert resp.status_code == 200

resp = client.post("http://localhost/post", json={"a": 1})
assert resp.json()["json"]["a"] == 1
resp = client.post("http://localhost/post", data={"a": "1"})
assert resp.json()["form"]["a"] == "1"
```

### C-2: Timeout Correctness

```python
with pytest.raises(reqx.ConnectTimeout):
    client.get("http://10.255.255.1/", timeout=reqx.Timeout(connect=0.5))

with pytest.raises(reqx.ReadTimeout):
    client.get("http://localhost/delay/5", timeout=reqx.Timeout(read=1.0))
```

### C-3: Concurrency Safety

```python
import threading
results, errors = [], []

def make_request():
    try:
        results.append(client.get("http://localhost/get").status_code)
    except Exception as e:
        errors.append(e)

threads = [threading.Thread(target=make_request) for _ in range(200)]
[t.start() for t in threads]
[t.join() for t in threads]

assert len(errors) == 0
assert all(r == 200 for r in results)
```

### C-4: Cookie Jar Behavior

```python
client = reqx.Client()
client.get("http://localhost/cookies/set?session=abc123")
resp = client.get("http://localhost/cookies")
assert resp.json()["cookies"]["session"] == "abc123"
```

### C-5: Streaming Body Correctness

```python
url = "http://localhost/bytes/65536"
resp_full = client.get(url)
full_body = resp_full.content

chunks = []
with client.stream("GET", url) as resp:
    for chunk in resp.iter_bytes(chunk_size=4096):
        chunks.append(chunk)

assert b"".join(chunks) == full_body
```

### C-6: Mock Transport

```python
class EchoTransport(reqx.MockTransport):
    def handle_request(self, request):
        return reqx.Response(200, json={"echo": str(request.url)})

client = reqx.Client(transport=EchoTransport())
resp = client.get("https://example.com/test")
assert resp.json()["echo"] == "https://example.com/test"
```

### C-7: Retry Correctness

```python
attempt_counts = {}

class FlakyTransport(reqx.MockTransport):
    def handle_request(self, request):
        url = str(request.url)
        attempt_counts[url] = attempt_counts.get(url, 0) + 1
        if attempt_counts[url] < 3:
            return reqx.Response(503, text="unavailable")
        return reqx.Response(200, json={"ok": True})

transport = reqx.HTTPTransport(
    retries=reqx.Retry(total=5, status_forcelist=[503], backoff_factor=0.0),
    _transport=FlakyTransport(),
)
client = reqx.Client(transport=transport)

resp = client.get("https://example.com/flaky")
assert resp.status_code == 200
assert resp.num_retries == 2
assert len(resp.retry_history) == 2
assert resp.retry_history[0].status_code == 503
assert resp.retry_history[1].status_code == 503
```

### C-8: Retry Budget Exhaustion

```python
class AlwaysFailTransport(reqx.MockTransport):
    def handle_request(self, request):
        return reqx.Response(503)

transport = reqx.HTTPTransport(
    retries=reqx.Retry(total=2, status_forcelist=[503], raise_on_status=True),
    _transport=AlwaysFailTransport(),
)
client = reqx.Client(transport=transport)

with pytest.raises(reqx.MaxRetriesExceeded) as exc_info:
    client.get("https://example.com/always-503")

e = exc_info.value
assert len(e.retry_history) == 2   # two retries, not three attempts
assert e.last_response.status_code == 503
```

### C-9: Retry-After Header Respect

```python
class RetryAfterTransport(reqx.MockTransport):
    def __init__(self):
        self.call_times = []

    def handle_request(self, request):
        self.call_times.append(time.monotonic())
        if len(self.call_times) < 3:
            return reqx.Response(429, headers={"Retry-After": "1"})
        return reqx.Response(200)

mock = RetryAfterTransport()
transport = reqx.HTTPTransport(
    retries=reqx.Retry(total=3, status_forcelist=[429], respect_retry_after_header=True),
    _transport=mock,
)
client = reqx.Client(transport=transport)
resp = client.get("https://example.com/")
assert resp.status_code == 200

# Each retry must have waited at least 1 second
gaps = [mock.call_times[i+1] - mock.call_times[i] for i in range(len(mock.call_times)-1)]
assert all(gap >= 0.9 for gap in gaps), f"Retry-After not respected: gaps={gaps}"
```

### C-10: Streaming Body Non-Retryability

```python
def generator_body():
    yield b"chunk1"
    yield b"chunk2"

transport = reqx.HTTPTransport(
    retries=reqx.Retry(total=3, status_forcelist=[503]),
    _transport=AlwaysFailTransport(),
)
client = reqx.Client(transport=transport)

# Streaming body must not be silently dropped or retried
with pytest.raises(reqx.ReqxError, match="cannot be retried"):
    client.post("https://example.com/upload", content=generator_body())
```

---

## Testing Requirements

### Minimum test coverage

| Checkpoint | Minimum passing tests |
|---|---|
| Checkpoint 1 | 10 |
| Checkpoint 2 | 50 |
| Checkpoint 3 | 80 |
| Checkpoint 4 | 110 |
| Final submission | 140 |

### Required test infrastructure

- `pytest` with `pytest-asyncio` and `pytest-anyio`
- A local `httpbin` Docker container as a test fixture (no reliance on external network for correctness tests)
- A `MockTransport` implementation usable in tests without a network
- All correctness tests (C-1 through C-10) must pass

---

## Implementation Constraints

**IC-1**: You may not vendor or fork `hyper`, `tokio`, `rustls`, or `pyo3`. Use them as Cargo dependencies at latest stable versions.

**IC-2**: You may not use `cffi`, `ctypes`, or `cython`. All FFI must go through PyO3.

**IC-3**: The Tokio runtime must be a singleton. You may not create a new runtime per client instance or per request.

**IC-4**: You may not use `asyncio.run_coroutine_threadsafe` or any Python-side mechanism to bridge async/sync. The sync client must use `runtime.block_on()` in Rust.

**IC-5**: All `unsafe` Rust must be justified with a comment explaining why it is necessary and why it is sound.

**IC-6**: The Python wrapper layer must be thin. No business logic in Python that could be in Rust. The Python layer handles: module re-exports, context manager `__enter__`/`__exit__`, kwargs normalization, and exception re-raising. Nothing else.

**IC-7**: The retry loop must execute entirely in Rust. No Python callbacks, no Python `time.sleep()`, no Python-side retry orchestration. The entire backoff-sleep-retry cycle must be a Rust `async` loop inside the transport layer.

**IC-8**: The `Cargo.toml` package name must be `reqx-core`. The Python package name is `reqx`. The PyO3 extension module is `_reqx`. Document this naming split clearly in your README.

---

## Codebase Requirements

### Directory structure

```
reqx/
├── Cargo.toml               # package name: reqx-core
├── Cargo.lock
├── pyproject.toml           # maturin config, Python package name: reqx
├── README.md
├── src/
│   ├── lib.rs               # #[pymodule] root, extension module name: _reqx
│   ├── client.rs            # PyClient
│   ├── request.rs           # PyRequest builder
│   ├── response.rs          # PyResponse, RetryRecord
│   ├── error.rs             # Rust→Python exception mapping
│   ├── runtime.rs           # Tokio singleton
│   ├── pool.rs              # Connection pool config
│   ├── retry.rs             # PyRetry, retry state machine, backoff logic
│   └── transport.rs         # PyHTTPTransport, PyAsyncHTTPTransport, MockTransport
├── python/
│   └── reqx/
│       ├── __init__.py
│       ├── _types.pyi       # type stubs
│       └── exceptions.py
├── tests/
│   ├── conftest.py
│   ├── test_sync.py
│   ├── test_async.py
│   ├── test_retry.py        # covers C-7 through C-10
│   ├── test_pool.py
│   ├── test_correctness.py  # C-1 through C-6
│   └── test_edge_cases.py
└── benchmarks/
    ├── README.md
    ├── bench_throughput.py
    ├── bench_latency.py
    ├── bench_memory.py
    ├── bench_json.py
    ├── bench_retry_overhead.py   # B-6
    └── bench_retry_throughput.py # B-7
```

### Code quality

- `cargo clippy -- -D warnings` must pass with zero warnings
- `ruff check python/` must pass with zero violations
- `mypy python/reqx/` must pass
- All public Rust types and functions must have doc comments
- All public Python classes and methods must have docstrings

---

## Final Report Requirements

The final report is 6–10 pages (excluding figures and appendices) and must contain:

**1. Design decisions (2 pages)**  
Cover the three most consequential choices: runtime singleton strategy, async bridging approach, and retry architecture. For retry specifically: where did you draw the boundary between Rust and Python? How did you handle the streaming body non-retryability constraint? What did you consider and reject?

**2. Performance results (2 pages)**  
Present all seven benchmarks (B-1 through B-7). Interpret B-6 and B-7 carefully — if retry overhead exceeds 100µs, explain why. If Rust-native retry does not outperform httpx + tenacity, explain the bottleneck.

**3. Correctness story (1 page)**  
What was hardest to get right? The Retry-After header parsing? The streaming non-retryability check? Backoff jitter correctness under concurrent requests sharing a `Retry` configuration object?

**4. Comparison to existing reqx Rust crate (0.5 pages)**  
You have now built something in the same space as the existing `reqx` Rust crate. Compare your retry design to theirs. What did they do differently? What would you adopt from their design in a v2?

**5. What remains (0.5 pages)**  
Specific gaps: unimplemented httpx features, missed performance targets, known failure modes.

**6. Individual contributions** (required for 3-person teams)

---

## Grading Rubric

| Component | Points |
|---|---|
| **Checkpoint 1** — Foundation | 10 |
| **Checkpoint 2** — Full sync/async | 15 |
| **Checkpoint 3** — Retry | 20 |
| **Checkpoint 4** — Advanced features | 15 |
| **Correctness** — C-1 through C-10 passing | 20 |
| **Benchmarks** — B-1 through B-7 reproducible, NFR-1 met | 15 |
| **Code quality** — clippy, ruff, mypy, IC compliance | 10 |
| **Final report** — analysis, depth, honest assessment | 15 |
| **Deductions** | |
| Rust panic propagates to Python as signal | −10 |
| Memory leak detected under valgrind | −5 per leak |
| GIL held during I/O or retry sleep | −10 |
| Retry loop orchestrated in Python (violates IC-7) | −15 |
| `unsafe` without justification | −3 per instance |
| Cargo package name is not `reqx-core` (violates IC-8) | −5 |
| Business logic in Python wrapper layer | −5 |
| **Total** | **120** (100 pts + 20 extra credit) |

---

## Suggested Weekly Timeline

| Week | Goal |
|---|---|
| 1 | Set up maturin build (`reqx-core`), hello-world PyO3 module, Tokio singleton |
| 2 | PyClient with sync GET, PyResponse with status/headers/body |
| 3 | GIL release verification, Checkpoint 1 tests, submit CP1 |
| 4 | Full sync client: all HTTP methods, request builder |
| 5 | pyo3-anyio integration, AsyncClient async methods |
| 6 | Exception hierarchy, timeout semantics, context managers |
| 7 | Checkpoint 2 tests (50), submit CP2 |
| 8 | Design retry architecture — write the design doc before any code |
| 9 | Implement `PyRetry`, backoff state machine, `RetryRecord` |
| 10 | Retry-After, streaming non-retryability, C-7 through C-10, submit CP3 |
| 11 | Connection pool config, HTTP/2, cookie jar |
| 12 | Streaming, MockTransport, proxy, certificate controls, submit CP4 |
| 13 | Benchmark harness (B-1 through B-7), performance tuning |
| 14 | Final report, code cleanup, demo prep, submit |

> **Week 8 note**: Do not write retry code before writing a design document. The retry state machine interacts with the response path, the error path, the timeout budget, and the GIL release strategy simultaneously. Debugging a poorly designed retry implementation under concurrent load is significantly harder than designing it correctly upfront. Your week 8 design document does not need to be long — a state diagram and a description of how backoff sleep interacts with GIL release is sufficient.

---

## Resources

### Required reading before starting

- [PyO3 User Guide](https://pyo3.rs/latest/) — especially chapters on classes, error handling, and the GIL
- [hyper documentation](https://docs.rs/hyper/latest/hyper/) — client, connection pooling
- [pyo3-anyio README](https://github.com/davidhewitt/pyo3-anyio) — async bridge strategy
- [reqx Rust crate docs](https://docs.rs/reqx/latest/reqx/) — study the retry and TLS backend design (do not copy)
- [urllib3 retry source](https://github.com/urllib3/urllib3/blob/main/src/urllib3/util/retry.py) — the reference implementation your API is based on
- Cloudflare blog: *How we built Pingora* — real-world precedent for this architecture
- httpx source: `httpx/_client.py`, `httpx/_transports/default.py` — understand what you're replacing

### Tooling

```bash
# Install maturin
pip install maturin

# Development install
maturin develop --release

# Build distributable wheel
maturin build --release

# Run httpbin locally
docker run -p 8080:80 kennethreitz/httpbin

# Profile Rust code from Python
py-spy record -o profile.svg -- python benchmarks/bench_throughput.py

# Verify GIL is released during I/O
python -c "
import threading, time, reqx
client = reqx.Client()
start = time.perf_counter()
threads = [threading.Thread(target=lambda: client.get('http://localhost/delay/1'))
           for _ in range(10)]
[t.start() for t in threads]
[t.join() for t in threads]
elapsed = time.perf_counter() - start
print(f'10 x 1s requests in {elapsed:.2f}s (should be ~1s if GIL released, ~10s if not)')
"
```

---

## Academic Integrity

All code must be written by your team. You may use public documentation, Stack Overflow, and GitHub issue trackers for reference. You may not copy code from other student projects, from the existing `reqx` Rust crate, or from any existing hyper/reqwest Python binding.

Using AI assistants is permitted for code generation, but you must be able to explain every line of your codebase during the demo. If you cannot explain code you submitted, that portion of the grade will be zeroed.

---

*Questions? Post to the course Ed Discussion board under the tag `[project-reqx]`. We will not answer questions about project requirements over email.*

*Spec v2.0 — April 2026. Changes from v1.0: added naming/prior art section, switched from reqwest to hyper as the underlying transport, added Phase 3 (Retry) as a standalone checkpoint, added FR retry requirements, added C-7 through C-10, added B-6 and B-7, added IC-7 and IC-8, updated grading rubric. The course staff reserves the right to issue clarification patches; material changes will be announced in lecture.*