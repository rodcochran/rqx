# v0.3.0

```bash
pip install --upgrade rqx
```

Streaming and response-surface release, and a breaking one. `stream()` is now a context manager on both clients, as in httpx: the request is sent when the block is entered and the connection released when it's left, and closing a response now stops any iterator still reading it. `chunk_size` is honored for the first time. The response surface picks up httpx's results where rqx had the same names and different behavior: `raise_for_status()` returns the response, `elapsed` is a `timedelta`, `json()` raises a `JSONDecodeError`, and misusing a stream raises a specific class. Every change below that alters existing behavior is listed under Behavior changes. A same-box benchmark A/B found no performance regression.

## Behavior changes

* `stream()` returns a context manager on both clients. `with client.stream(...) as resp:` and `async with client.stream(...) as resp:` send the request on entry and close the response on exit. `await client.stream(...)` no longer works, and calling `stream()` without `with` sends nothing. The request is built at the call, so a bad URL or header raises there (#186, closes #84).
* Stream responses are no longer context managers themselves; the context `stream()` returns is. `close()` and `aclose()` remain (#186).
* Closing a response stops its iterators. An iterator kept past the `with` block, or still running when `close()` is called, raises `StreamClosed` on its next chunk instead of reading on. Closing also interrupts a read that's waiting on the network (#186, #187).
* `iter_bytes(chunk_size)` and `iter_text(chunk_size)` return exactly `chunk_size` bytes or characters per piece, except the last. The argument was accepted and ignored before. Without it, chunks pass through as the network delivers them, as in httpx. `chunk_size=0` raises `ValueError` (#187, closes #107).
* `iter_lines()` and `aiter_lines()` take no `chunk_size`, matching httpx (#187).
* An exhausted iterator keeps raising `StopIteration` / `StopAsyncIteration` instead of an error on a further call (#187).
* `elapsed` is a `datetime.timedelta` on all three response classes, and on buffered responses it now stops when the headers arrive rather than after the body is read, the same as streamed responses (#193, closes #189). Use `elapsed.total_seconds()` where a float was expected.
* `response.json()` on a body that isn't JSON raises `rqx.JSONDecodeError`, which is both an `rqx.RqxError` and a stdlib `json.JSONDecodeError`, with `doc`, `pos`, `lineno` and `colno` positioned like the stdlib parser's (#193, closes #189).
* `HTTPStatusError` carries the response as `.response`, and its message matches httpx's word for word, including the redirect location and the MDN link (#193, closes #189).
* Misusing a stream raises `StreamConsumed` (read or iterated twice), `StreamClosed` (used after close) or `ResponseNotRead` (`.content`, `.text` or `.json()` before `read()`), all under `rqx.StreamError`, which is both an `RqxError` and a `RuntimeError`. These were bare `RqxError`s. Iterating a body that `read()` already buffered raises `StreamConsumed`; httpx allows it, rqx consumes a body once (#193, closes #190).

## Additions

* `raise_for_status()` returns the response on 2xx, so it chains: `client.get(url).raise_for_status().json()`. Stream responses gain the method (#193, closes #189).
* `chunk_size` on the byte and text iterators, with memory bounded at one network chunk plus `chunk_size` (#187, closes #107).
* `rqx.JSONDecodeError`, `rqx.StreamError`, `rqx.StreamConsumed`, `rqx.StreamClosed`, `rqx.ResponseNotRead` (#193, closes #190).

## Performance

Full run on paired AWS `c7i.large` instances (client + nginx, single-AZ), rqx at `c5f1305`, 5 runs per bench, against httpr 0.7.2, aiohttp 3.14.3, httpx 0.28.1 — the same comparator versions as 0.2.0. Charts in [`benchmarks/0.3.0/`](https://github.com/rodcochran/rqx/tree/v0.3.0/benchmarks/0.3.0); tables, method, the A/B and limitations in [`benchmarks/0.3.0/report.md`](https://github.com/rodcochran/rqx/blob/v0.3.0/benchmarks/0.3.0/report.md).

* **No regression, confirmed on the same machines.** Against the 0.2.0 run rqx rose 12–28% from c=50 up, in line with the controls, but was flat at c=10 while they rose. A same-box A/B against `main` without this release's response changes put the two builds within 1% once the machine had warmed up; the c=10 gap is the instance pair.
* **Throughput (b1):** rqx leads every client at every concurrency — +25% over httpr and +61% over aiohttp at c=100.
* **Latency (b2, c=100):** p50 4.78 ms, lowest of the four; p99 10.70 ms, above aiohttp's 7.87 — the #168 tail, unchanged.
* **Memory:** within 1.9 MB of 0.2.0 at every concurrency.
* **httpx comparison corrected.** The b1 harness now warms up on the client it measures (#192, closes #188). httpx gains 2–4× at c=500 and c=1000 from that alone, so rqx's lead over httpx there is about 45×, not the 100×+ earlier reports showed. The earlier reports also misattributed httpx's low numbers to its default pool limits; the benches set 1,500, and the cause is httpcore re-scanning its pool on every request. Those reports now carry a correction.
* **Stability:** zero aborts across 95 b1 cells, 5 b2 runs, 5 b8 runs; zero request failures. Fourth consecutive clean run.

## Internals

* `PyStreamContext` / `PyAsyncStreamContext` hold a built request and send it once on entry (#186).
* `LiveStream`: the response and its one iterator share a handle to the body, with a closed flag and a wake-up so a close interrupts a pending read (#186, #187).
* `ByteChunker` and `TextChunker` regroup network chunks; whole pieces are split off without copying (#187).
* Five exception classes with a stdlib base as well as an rqx one are built with `type()` at module init and raised through `import_exception!`, so raise sites in async code need no GIL (#193).
* A buffered request now goes through the same send as `stream()` before reading, which is where `elapsed` is stamped (#193).

## Tests

* Context-manager, iterator-close, chunking, memory-bound and stream-exception suites, sync and async; a hypothesis property over body length and chunk size (#186, #187, #193).
* `tests/unit/streaming/test_memory_bound.py` streams 128 MB through a deliberately slow consumer and bounds peak RSS; it's the reason streaming memory has no benchmark (#187).
* Equivalence cases against httpx for the stream context, stream exceptions, `raise_for_status()`, `elapsed`, `json()` errors and the exact status-error message (#186, #193).
* The stub-vs-runtime check covers the new classes, including dotted stdlib bases (#193).
* Fixture routes `/large/<mib>`, `/bytes/<n>`, `/slow-body/<seconds>`, and a canned server that can stall a response mid-body (#187, #193).

## Benchmarks & docs

* The benchmark reports for 0.1.4, 0.1.5 and 0.2.0 carry a dated correction of the httpx explanation (#192, closes #188).
* README and CONTRIBUTING corrected: the API is httpx-familiar, not a drop-in replacement; setup, the Python floor (3.8) and the test layout were out of date (#192).
* b1 warms up on the client it measures, and b1 and b4 report memory correctly on both macOS and Linux (#192).
* `just benchmarks::local-up` and the AWS server setup generate the 10 KB and 100 KB payloads compose mounts, and clear the empty directories Docker leaves when they're missing (#192).
* `stream_ab` harness updated to the `async with` stream form (#187).

## Known issues

* #176 — building a `Client` costs ~35–50 ms on Linux, and the module-level verbs build one per call. Unfixed.
* #168 — tail latency under load, unchanged.
* #167 — retries cover failures up to the response headers only, unchanged; scheduled for v0.5.0.

## Deferred

* v0.4.0, the last API-shaping release: middleware (#147) with event hooks as its first built-in (#19), the `Request` / `Response` / `URL` model it needs (#28, #27, #59), `MockTransport` (#24) and `Auth` with digest (#26), stub audit (#33).
* v0.5.0, a non-breaking soak: running existing httpx codebases on rqx (#191), body-read retries (#167), the migration and divergence guide (#116).
