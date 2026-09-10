# v0.1.4

```bash
pip install --upgrade rqx
```

Correctness release. Fixes the runtime lifecycle so rqx survives `fork()` and interpreter shutdown, makes the retry policy apply to every send path (redirect hops, streaming, per-failure-kind caps), and carries request bodies across 307/308 redirects. Builds on a response-model refactor that collapses the buffered and streaming paths onto one type. No breaking API changes; two behavior changes are called out below. Throughput leads every comparison client at every concurrency on a fresh full bench run; the shutdown abort seen on the last two bench runs did not recur.

## Fixes

* Build the tokio runtime on first use instead of at import, and rebuild it per process. A prefork server (gunicorn, `uvicorn --workers`) or `multiprocessing` child that inherited the import-time runtime hung or aborted on its first request; `import rqx` now starts no threads (#162, closes #159).
* Shut the runtime down in an `atexit` hook before interpreter finalization. An in-flight async result delivered during finalization could abort the process (#162, closes #99).
* On macOS, register an `os.register_at_fork` hook that initializes the Apple frameworks a client build touches while still in the parent, so the forked child does not trip the Objective-C fork guard on its first request (#162).
* Apply the retry policy under `follow_redirects=True`. The redirect loop called the transport's single-attempt path directly and bypassed `send_with_retries` entirely (#160, closes #148).
* Apply the retry policy to `client.stream(...)`. Streaming never entered the retry loop, with or without redirects (#160).
* Preserve the request body across 307 and 308 redirects. A `POST` through a 307 arrived body-less. 302 and 303 now drop the body together with `Content-Type`, `Content-Length`, and `Transfer-Encoding` on the downgrade to GET, matching httpx (#165, closes #149).
* Resolve relative `Location` headers against the hop that sent them rather than the original URL. Multi-hop chains with relative redirects went to the wrong path (#165).
* Enforce `Retry.connect`, `Retry.read`, and `Retry.status` as independent caps. They were accepted and ignored; every retry counted only against `total`. Each kind has its own cap and every retry also counts against `total`; unset caps default to `total`, so existing configs behave as before. `MaxRetriesExceeded` now reports the breakdown, e.g. `max retries exceeded after 3 retries (0 connect, 1 read, 2 status)` (#166, closes #54).
* Remove one copy per chunk from the streaming iterators (#139, closes #108).

## Behavior changes

* Streaming requests honor the transport's retry policy. A streamed request that previously made exactly one attempt now retries like a buffered one; set `retries=None` on the transport to keep single-attempt behavior.
* Under `follow_redirects`, retry caps apply per hop: a 503 on the third hop retries the third hop. `num_retries` and `retry_history` on the final response are cumulative across the chain.
* `elapsed` on a stream response is set when headers arrive and does not change after the body is read. Buffered responses still measure through the body read.

## Performance

Full run on paired AWS `c7i.large` instances (client + nginx, single-AZ), rqx at the release commit, 5 runs per bench, against httpr 0.7.2, aiohttp 3.14.3, httpx 0.28.1. Charts in [`benchmarks/0.1.4/`](https://github.com/rodcochran/rqx/tree/v0.1.4/benchmarks/0.1.4); tables, method, and limitations in [`benchmarks/0.1.4/report.md`](https://github.com/rodcochran/rqx/blob/v0.1.4/benchmarks/0.1.4/report.md); raw logs in [`benchmarks/results/aws-20260910-v014/`](https://github.com/rodcochran/rqx/tree/v0.1.4/benchmarks/results/aws-20260910-v014).

* **Throughput (b1):** rqx leads every client at every concurrency — +29% over httpr and +69% over aiohttp at c=100 (19,723 RPS, median of 5, ±1%). Versus the 0.1.3 run rqx is +8 to +14%, but the box is newer (kernel 7.0, rustc 1.98.1) and the unchanged clients moved +1 to +12% too; the code-attributable gain is the same-box A/B on #162: +1.6 to +4.4% with the controls flat.
* **Memory (b1):** rqx peak RSS down 14–16% at c=500–1000 (77.5 → 66.0 MB, 101.0 → 84.9 MB); lightest client through c=50. At c=500+ aiohttp and the current httpr are lighter.
* **Latency (b2, c=100):** p50 **4.79 ms** (was 5.48), lowest of the four; p99 12.33 ms, unchanged, and higher than aiohttp's 8.52 — see #168.
* **Latency under load (b8):** lowest p50 at every concurrency; p99/p50 ratio 1.9–2.1×, unchanged from 0.1.3; aiohttp holds 1.0–1.2×.
* **Stability:** zero aborts across 95 b1 cells (aiohttp c=1000 is skipped by design), 5 b2 runs, 5 b8 runs. The 0.1.2 and 0.1.3 bench runs each hit the shutdown abort (#99); this one did not.
* httpr moved from 0.4.8 to 0.7.2 between runs and its `AsyncClient` no longer self-throttles to asyncio's default executor, so this run's rqx-vs-httpr lead is the fair one; the 0.1.3 report's "3.4× lighter than httpr" no longer applies.

## Internals

* Introduce `PendingResponse` (`src/response.rs`) — headers received, body unread. `Transport::send` returns it for every path; `Client::request` reads it, `Client::stream` hands the live body to the stream response. Redirects, retries, and cookie accumulation operate on this one type; the parallel buffered/streaming implementations and `Transport::handle_request` are gone (#164, closes #55).
* Factor `Client::request` / `Client::stream` onto a shared `Client::build` + `Client::send`. `Client::send` is the pre-body hook point that event hooks (#19) and middleware (#147) need (#164).
* Introduce `RequestSpec` (`src/request.rs`) — a prototype request cloned per attempt and per redirect hop via `Request::try_clone`, which shares the body by reference count. Replaces `build_redirect_request` and the mid-loop `try_clone` error path (#165).
* Own the tokio runtime in `src/runtime.rs`: a lock-free PID-checked slot, a `Bridge` implementing pyo3-async-runtimes' `Runtime` + `ContextExt` so the async path never depends on that crate's set-once global, and a bounded `shutdown_timeout` at exit (#162).
* Add `FailureKind` and `RetryCounts` (`src/retry.rs`); `Transport::execute` returns the raw `reqwest::Error` so the loop classifies a failure before mapping it (#166).
* Bump pyo3 0.28.3 → 0.29.2, pyo3-async-runtimes 0.28.0 → 0.29.0, tokio → 1.53.1; drop the unused `async-std` feature (#162).

## Tests

* New `tests/test_runtime_lifecycle.py` — import thread count, fork survival (cold and warm parent, sync and async child), in-flight future at exit, sustained async load then exit, cancelled requests then exit. Each case runs in a subprocess (#162).
* New `tests/test_redirect_body.py` — method and body across 301/302/303/307/308 for JSON, form, and raw bodies; body on retried requests; relative `Location` across a nested chain; async variants (#165).
* `tests/test_retry_config.py` — retries under redirects (the two `xfail`s from #148 now pass), streaming retries, per-hop cap scope, and per-kind caps: each cap alone, `total` capping a generous kind, zero caps, mixed kinds in one request (#160, #166).
* `flaky_server` fixture gains `/redirect-to-flaky`, `/flaky-redirect`, `/echo-body`, `/redirect/<status>`, `/flaky-echo-body`, `/reset-then-flaky`, and a nested relative-redirect chain; PUT/PATCH share the POST routes (#160, #165, #166).
* Fix the flaky mTLS test by generating certificates once per run in `pytest_configure`, before xdist workers start (#144, closes #112).

## CI & packaging

* Dependabot (27): `rand` 0.8 → 0.10 (#73, #137, #142), `encoding_rs` 0.8.40 (#161), `quinn-proto` 0.11.16 (#146), `rustls-webpki` 0.103.13 (#65), the rust minor/patch groups (#126, #141, #156), `actions/checkout` 7 (#125), `actions/setup-python` 7 (#140), and the `benchmarks/infra` npm tree (#124, #129, #130, #132–#135, #138, #145, #152–#155, #157, #158, #163).

## Known issues

* #167 — retries cover failures up to the response headers only; a body that truncates after a 200 is not retried. Body-phase retries need a buffered-only policy above the transport and are tied to the middleware seam (#147).
* 301 preserves the request method where browsers, requests, and httpx downgrade to GET. Pinned by a test as current behavior; the decision is open (#149).
* #168 — tail latency under load: each async completion takes the GIL from a tokio blocking thread, so p99 climbs in ~5 ms steps above p50 at high concurrency (p50 4.8 ms / p99 12.3 ms at c=100). Unchanged from 0.1.3; the fix is a batched delivery path drained on the loop thread.

## Deferred

* The v1.0.0 milestone's httpx-parity items (#84, #88, #114, #115, #117, #118) and the docs set (#33, #36, #37, #116) did not land in this release.
