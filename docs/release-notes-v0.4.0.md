# v0.4.0

```bash
pip install --upgrade rqx
```

URLs, query parameters and client configuration, and a breaking release. `rqx.URL` and `rqx.QueryParams` arrive with httpx's semantics, and `response.url` and `client.base_url` now return a `URL` instead of a string. `params=` now follows httpx: `None` sends an empty value, list values repeat the key, and passing `params=` replaces any query already on the URL. Redirect settings move off `Retry` into a new `rqx.RedirectPolicy`. A per-request `auth=` now replaces a client-level `auth_bearer` instead of raising. Every change below that alters existing behavior is listed under Behavior changes. Throughput is up 7–14% against 0.3.0, mostly from the fat-LTO build that accompanies the crate split; peak memory at c=500 and above is up 9–11 MB, a known regression listed under Performance.

## Behavior changes

* `response.url` and `client.base_url` return `rqx.URL`, not `str`. Comparing with `==` against a string still works; string methods such as `.endswith(...)` need `str(url)` first (#196, closes #59).
* `params=` with a `None` value sends the key with an empty value (`a=`) instead of dropping it, as in httpx (#196).
* `params=` replaces a query already on the URL instead of appending to it, as in httpx (#196).
* `params=` values that are a list or tuple repeat the key once per element (#196).
* `Retry(raise_on_redirect=...)` and `Retry.raise_on_redirect` are removed. Use `RedirectPolicy(raise_on_exceeded=...)`, passed as `Client(redirects=...)` (#204).
* A per-request `auth=` replaces a client-level `auth_bearer` for that request. It used to raise, because the client's bearer token and the request's basic auth were treated as a collision (#204).
* Passing both `auth=` and `auth_bearer=` in the same call raises `ValueError`. It used to raise `rqx.RequestError`, so `except rqx.RqxError` no longer catches it (#204).
* `Client(redirects=...)` raises `rqx.RqxError` when `follow_redirects=` or `max_redirects=` is also passed with a different value. Passing the same value in both places is accepted (#204).
* `repr(rqx.Timeout(...))` spells values the way Python does: `Timeout(connect=5.0, read=None, ...)` rather than `Some(5.0)` (#204).

## Additions

* `rqx.URL`: parse, inspect (`scheme`, `host`, `port`, `path`, `query`, `params`, `fragment`, ...), `join`, and copy-style updates (`copy_with`, `copy_set_param`, `copy_add_param`, `copy_remove_param`, `copy_merge_params`). It holds relative references as written. `repr()` masks the password (#196, closes #59).
* `rqx.QueryParams`: an immutable multi-dict with repeated keys, built from a mapping, a sequence of pairs, a `str` or `bytes`, with `set`, `add`, `remove` and `merge` returning new instances (#196).
* `rqx.InvalidURL`, which is both an `rqx.RqxError` and a `ValueError` (#196).
* `url=` accepts a `URL` as well as a string, and `params=` accepts `QueryParams`, a sequence of pairs, `str` or `bytes`, on sync, async and streaming calls (#196).
* `rqx.RedirectPolicy(follow=, max_redirects=, raise_on_exceeded=)`, accepted as `Client(redirects=...)` and `AsyncClient(redirects=...)` and readable back as `client.redirects`. `follow_redirects=` and `max_redirects=` on the client still work (#204).

## Performance

* **URL and QueryParams (#196):** interleaved A/B against `main` on the AWS pair found no regression in throughput, latency or memory. Two apparent regressions were measurement artifacts: an RSS difference caused by build order, and a −4.7% at c=500 from three runs per side that 20 alternating pairs measured at −0.6%. Measured costs: +80 bytes per live response and +147 KB of module text.
* **Multi-crate split (#202):** release builds now use fat LTO, which restores the cross-crate inlining the split removed. Measured 2026-09-22: +4% throughput and a 17% smaller wheel against v0.3.0.
* **Configuration refactor (#204):** same-box A/B against `main` (`15ee73b` vs `8884722`), 20 alternating pairs per concurrency on paired AWS `c7i.large` instances:

  | c | main rps | #204 rps | median Δ | #204 faster | RSS main → #204 |
  |---|---|---|---|---|---|
  | 10 | 19,042 | 19,005 | −0.45% | 7/20 | 28.4 → 28.3 MB |
  | 100 | 20,298 | 20,253 | +0.31% | 11/20 | 41.4 → 41.0 MB |
  | 500 | 19,769 | 19,741 | −0.05% | 10/20 | 75.3 → 74.6 MB |

  All within the noise floor. The httpr and aiohttp controls drifted by 3–4% during the session, which the alternating order cancels.

Full run on paired AWS `c7i.large` instances (client + nginx, single-AZ), rqx at `a7c7f33`, 5 runs per bench, against httpr 0.7.2, aiohttp 3.14.3, httpx 0.28.1 — the same comparator versions as 0.3.0. Charts in [`benchmarks/0.4.0/`](https://github.com/rodcochran/rqx/tree/v0.4.0/benchmarks/0.4.0); tables, method, the A/Bs and limitations in [`benchmarks/0.4.0/report.md`](https://github.com/rodcochran/rqx/blob/v0.4.0/benchmarks/0.4.0/report.md).

* **Throughput (b1):** up 7–14% against 0.3.0, 3.5–7 points more than the httpr and aiohttp controls rose on this faster instance pair. That margin is in line with the fat-LTO A/B above. rqx leads every client at every concurrency — +31% over httpr and +67% over aiohttp at c=100, 52× httpx at c=1000.
* **Latency (b2, c=100):** p50 4.39 ms (−8.2% against 0.3.0, controls −1 to −2%), lowest of the four; p99 10.20 ms, above aiohttp's 7.65 — the #168 tail, unchanged.
* **Memory — known regression:** peak RSS at c=500 and c=1000 is up 8.9 MB and 10.6 MB (+13–14%) against 0.3.0, with every other client within 0.5 MB. Low concurrency is unchanged or lower (c=10 −1.3 MB). It came in with #196 or #202, not with dependency bumps or #204, and is not yet bisected.
* **Stability:** zero aborts across 95 b1 cells, 5 b2 runs, 5 b8 runs; zero request failures. Fifth consecutive clean run.

## Internals

* The code is split into two crates: `rqx-core`, pure Rust with no pyo3, and `rqx`, the Python binding (#202).
* `ClientConfig` groups a client's timeout, redirect policy, base URL and default auth, so `Client::new` takes a transport and a config instead of six arguments (#204).
* `Auth::{None, Basic, Bearer}` replaces the separate basic and bearer arguments. The basic-vs-bearer rule is checked when an `Auth` is built (#204).
* `RedirectPolicy` holds `follow`, `max_redirects` and `raise_on_exceeded`, which removes the redirect loop's read of `raise_on_redirect` from the transport's retry config (#204).
* The seven per-verb methods on the core `Client` are removed. The binding's verbs call `request` (#204).
* A `PyRepr` trait spells `bool`, `u32`, `f64` and `Option<T>` the way Python's `repr()` does, without the GIL (#204).

## Tests

* URL and QueryParams suites, with equivalence cases against httpx and RFC 3986 join cases (#196, #201).
* Redirect policy: raising versus returning the last 3xx, conflicting and agreeing client kwargs, getters and `repr`, sync and async (#204).
* Rust unit tests for `PyRepr`, and a hypothesis property checking `repr(Timeout)` and `repr(RedirectPolicy)` against Python's own `repr` for any value (#204).
* CI runs `cargo test --workspace` (was `--lib`), matching the new `just test-rust`, which `just check` now includes (#204).
* The test suite floors httpx at 0.28 (#201).

## Benchmarks & tooling

* `benchmarks/ab_report.py` labels builds from the run's `metadata.txt`: short SHA, plus the branch name when the SHA is a branch tip. It previously used hardcoded labels from an earlier A/B (#204).

## Dependencies

* Rust minor and patch bumps, including `rand` 0.10.3 (#197, #203). Python test and dev dependencies `anyio` 4.14.2 and `h11` 0.16.0 (#198, #199).
