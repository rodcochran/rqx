# v0.1.5

```bash
pip install --upgrade rqx
```

Request-building release. The three per-request inputs that ported httpx code trips over — `params=`, `headers=`, and `json=` — now accept what httpx accepts, validate at the call boundary, and raise a Python exception instead of panicking. Fixes one crash: any list inside a `json=` body took the interpreter down. Two behavior changes are called out below; neither changes an API signature. Also the first release with a property-based suite and an httpx equivalence suite, which is what found the crash and a float-rounding bug on decode.

## Fixes

* Accept `int`, `float`, `bool`, and `None` values in `params=`, coerced the way httpx does it: `True` → `true`, `None` drops the key, floats keep Python's `str()` spelling. Dict order is the wire order. Unsupported value types and non-str keys raise `TypeError` (#171, closes #115).
* Build request headers straight into a `HeaderMap` at the call boundary. An invalid header name or value raised a `PanicException`; it now raises `ValueError` naming the offending text. Same for a malformed method string. `headers=` also accepts an `rqx.Headers` instance, any mapping with `items()`, and case-variant keys become separate header lines like httpx. Methods are uppercased before validation, so `request("get", ...)` sends `GET` (#173, closes #117).
* Reject header maps past `HeaderMap`'s entry limit with `ValueError` instead of a panic, on the request path and on the `Headers` class (#173).
* Encode `json=` like stdlib `json.dumps`: tuples are arrays, dict keys coerce (`True` → `"true"`, `None` → `"null"`, numbers via `str()`), NaN and infinities raise `ValueError`, unsupported types raise `TypeError` with stdlib's wording, ints past 64 bits raise `OverflowError`, and a self-referencing structure raises `ValueError` instead of overflowing the stack (#174, closes #118).
* **Fix a crash: any list anywhere in a `json=` body segfaulted the interpreter.** The list arm of the encoder iterated its own `Result` wrapper and recursed on the list itself. Present in every published wheel before this one (#174).
* Decode JSON integers between 2^63 and 2^64 − 1 exactly. They came back as rounded floats (#174).
* Decode JSON floats correctly rounded. serde_json's default float parser is best-effort and hypothesis found a literal that came back one ulp off; `float_roundtrip` is now enabled (#175).
* `import rqx` works on Python 3.8 and 3.9 again. Module-level type aliases in `_api.py` used `|` and PEP 585 generics at import time; cp38/cp39 wheels shipped broken (#171).

## Behavior changes

* Dict order is preserved on the wire for `json=` bodies and in `response.json()`. Both were sorted by key (serde_json `preserve_order`) (#174).
* A `POST` through a 301 follows as a body-less `GET`, matching httpx and browsers. 302 downgrades every method except `HEAD`; 301 downgrades only `POST`; 307 and 308 keep the method. Previously 301 preserved the method (#179).
* `json=` inputs that used to be sent as `null` — NaN, infinities, `datetime`, `bytes`, `set`, arbitrary objects — now raise (#174).

## Performance

Full run on paired AWS `c7i.large` instances (client + nginx, single-AZ), rqx at the release commit, 5 runs per bench, against httpr 0.7.2, aiohttp 3.14.3, httpx 0.28.1 — the same comparator versions as the 0.1.4 run. Charts in [`benchmarks/0.1.5/`](https://github.com/rodcochran/rqx/tree/v0.1.5/benchmarks/0.1.5); tables, method, and limitations in [`benchmarks/0.1.5/report.md`](https://github.com/rodcochran/rqx/blob/v0.1.5/benchmarks/0.1.5/report.md); raw logs in [`benchmarks/results/aws-20260911-v015/`](https://github.com/rodcochran/rqx/tree/v0.1.5/benchmarks/results/aws-20260911-v015).

* **Neutral by design.** This release changed request validation and encoding at the call boundary and nothing on the send path. At the chart cell (c=100) rqx serves 19,743 RPS vs 19,723 in 0.1.4; peak RSS matches at every concurrency to within half a megabyte; b2 p50 is 4.81 ms vs 4.79.
* **Throughput (b1):** rqx leads every client at every concurrency — +26% over httpr and +64% over aiohttp at c=100. The +7–8% over 0.1.4 at c=500–1000 is the instance: httpr and aiohttp moved +10 to +16% at the same levels. c=10 is −4.7% against controls at −1.4% to −5.7%, inside the band but at its edge; a same-box A/B would settle whether any of it is the new per-request validation.
* **Memory (b1):** unchanged — 29.3 / 33.8 / 38.6 / 66.5 / 85.0 MB across c=10…1000. Lightest client through c=50.
* **Latency (b2, c=100):** p50 4.81 ms, lowest of the four; p99 12.11 ms, still above aiohttp's 8.71 — the #168 tail, unchanged.
* **Stability:** zero aborts across 95 b1 cells, 5 b2 runs, 5 b8 runs; zero request failures. Second consecutive clean run since the 0.1.4 lifecycle fix.

## Internals

* `QueryParams` (`src/query_params.rs`), `RequestHeaders` (`src/request_headers.rs`), and `JsonBody` (`src/py_json.rs`): one newtype per request input, each with its own `FromPyObject`, so validation and coercion happen at the pyo3 boundary and the verbs take typed arguments (#171, #173, #174).
* `determine_redirect_method` now encodes httpx's table exactly (#179).

## Tests

* Test suite split into `tests/unit/`, `tests/integration/`, `tests/property/`, and `tests/equivalence/` packages, with `just test-unit` / `test-integration` / `test-property` / `test-equivalence` recipes and `integration` / `equivalence` markers (#175, #179).
* Property-based suite with hypothesis: params round trip, header round trip and validation, JSON encode parity with stdlib and decode round trip, base URL merging. 100 examples per property by default, `HYPOTHESIS_PROFILE=nightly` for 1000. Retry counters are covered by exhaustive Rust tests instead (#175, closes #44).
* Equivalence suite: each test runs against `httpx.Client` and `rqx.Client`. Known divergences are strict xfails carrying their issue link, so a divergence that closes fails the test until the mark is removed (#179, closes #42).
* Integration tests start `ghcr.io/psf/httpbin:0.10.2` themselves via testcontainers; `RQX_HTTPBIN_URL` points them at an existing server. `just httpbin-start` and the CI service container are gone (#175).
* Fixture servers are threaded, run with `TCP_NODELAY`, and stay quiet about clients that gave up. Sleep-based tests use 0.2 s timeouts. Local suite: 9.8 s → 4.2 s for 531 tests; `pytest-timeout` caps any test at 30 s (#175).
* Fixture routes added: `/echo-headers`, `/echo-url/*`, `/reflect-json`, `/big-ints`, `/cookies`, `/cookies/set` (#173, #174, #175, #179).

## CI & packaging

* Dev tooling moved from the public `rqx[dev]` extra to PEP 735 dependency groups: `test`, `lint`, and `dev`. `rqx[benchmarks]` remains (#175).
* Test job: 218 s → 57 s. Rust cache, `uv sync --no-install-project` (the sync was building a release wheel that the next step replaced), `uv run --no-sync`, and the httpbin image pulled in the background (#175).
* `just lint` runs the locked ruff via `uv run` and covers `tests/` (#173).
* Dependabot: `pillow` 12.3.0 (#172).
* Bench `client-setup.sh` installs maturin explicitly; it relied on the removed `dev` extra and the first v0.1.5 bench attempt failed in setup.

## Known issues

* #176 — building a `Client` costs ~35–50 ms on Linux (reqwest's default TLS backend loads the system CA store per client), and the module-level verbs build one per call. Found by the property suite; unfixed.
* #168 — tail latency under load, unchanged.
* #167 — retries cover failures up to the response headers only, unchanged.
* The Actions cache is at its 10 GB cap with sccache fragments from the wheel matrix, evicting the test job's Rust cache; new branches build cold.

## Deferred

* v0.2.0: the exception tree (#114, #88, #53). v0.3.0: `stream()` as an async context manager (#84), backpressure (#107), middleware (#147, #19). v1.0.0: LTO (#119) and the docs set (#116, #37, #36).
