# v0.2.0

```bash
pip install --upgrade rqx
```

Exception release, and a breaking one, which is why the minor version moves. The hierarchy now has httpx's shape: `RequestError` and `HTTPStatusError` are siblings under a new `HTTPError` base, so `except RequestError` no longer catches a 404 from `raise_for_status()` and `except HTTPError` catches everything a request can raise. Four classes join the tree (`RemoteProtocolError`, `DecodingError`, `UnsupportedProtocol`, and a `ProtocolError` parent) and `ProxyError` is finally raised, each by a real failure that used to land in the generic `RqxError` or the wrong leaf. Porting from httpx is a rename of the exception imports; rqx's exceptions are not subclasses of httpx's, and that is a decision, not a gap (see Deferred). Also the first release whose benchmark ran end to end through the new bench tooling.

## Behavior changes

* `HTTPStatusError` is no longer a subclass of `RequestError`. Both are children of the new `HTTPError`, as in httpx. Code that relied on `except RequestError` swallowing status errors must catch `HTTPError` or `HTTPStatusError` explicitly. The stub said this all along; the runtime disagreed (#183, closes #114).
* `MaxRetriesExceeded` moved under `HTTPError` as a third sibling, so the catch-all covers retry exhaustion too (#183).
* Failures that fell through to a bare `RqxError` now raise `RequestError` or a specific leaf, so `except HTTPError` covers them: malformed response heads, servers hanging up before or during the headers, bad URLs, and every request-building failure (#183).
* A server that breaks HTTP raises `RemoteProtocolError`: garbage status line, invalid header line, connection closed before the message completed, body shorter than `Content-Length`, bad chunk size. These were `RqxError` or `ReadError` (#183, closes #182).
* A body the decompressor rejects (gzip, br, zstd, deflate) raises `DecodingError`, a `RequestError` but not a `TransportError`. It was `ReadError` (#183).
* A TCP reset before or during the body raises `ReadError`; before the response it was the generic fallback (#183).
* A URL with no scheme, a relative path with no `base_url`, or a non-http scheme (`ftp://`, `file://`, `mailto:`) raises `UnsupportedProtocol` with httpx's wording. An unparsable URL raises `ValueError`, matching `base_url`. Both were `RqxError("Failed to build request: builder error")` (#183).
* A proxy that answers the CONNECT with a failure (503, 407) raises `ProxyError`. An unreachable proxy stays `ConnectError`, and a proxy's reply to a plain `http://` target is returned as the response. All three match httpx; the first was `ConnectError` (#183, closes #53 via #182).
* Streaming iterators (`iter_bytes`, `iter_text`, `iter_lines` and the async three) raise the same classes as a buffered read. They formatted every body error into a bare `RqxError("stream error: …")` (#183).

## Fixes

* The exception tree in `python/rqx/_types.pyi` matches the compiled module, and a test walks the stub's class tree and checks every parent against the runtime, so they cannot drift again (#183, closes #114).
* `ProxyError` is raised. It existed since 0.1.0 and nothing constructed it (#183, closes #53).

## Performance

Full run on paired AWS `c7i.large` instances (client + nginx, single-AZ), rqx at `bd8df2f`, 5 runs per bench, against httpr 0.7.2, aiohttp 3.14.3, httpx 0.28.1 — the same comparator versions as the 0.1.5 run. Charts in [`benchmarks/0.2.0/`](https://github.com/rodcochran/rqx/tree/v0.2.0/benchmarks/0.2.0); tables, method, and limitations in [`benchmarks/0.2.0/report.md`](https://github.com/rodcochran/rqx/blob/v0.2.0/benchmarks/0.2.0/report.md); raw logs archived from run `20260912-182933`.

* **Neutral by design.** This release changed error classification after a failure and nothing on the send path. rqx's lead over httpr is +35 / +32 / +31 / +21 / +22% at c=10…1000, against +32 / +31 / +26 / +19 / +23% in 0.1.5; peak RSS matches 0.1.5 within 1.2 MB at every concurrency.
* **A slower instance pair.** Every absolute number is 7–25% below the 0.1.5 run for every client, httpr and aiohttp included, so the box moved and the code did not. The report leads with the controls table for that reason.
* **Latency (b2, c=100):** p50 5.36 ms, lowest of the four; p99 12.70 ms, still above aiohttp's 10.04 — the #168 tail, unchanged.
* **Stability:** zero aborts across 95 b1 cells, 5 b2 runs, 5 b8 runs; zero request failures. Third consecutive clean run.

## Internals

* `map_reqwest_error` reads the error's source chain where reqwest's predicates lump failures together: a `hyper::Error` underneath decides protocol versus network (parse or incomplete message, or a body framing error with no OS error beneath it, is the server's fault), a decode error with no transport error beneath it is the decompressor, and the proxy tunnel is recognized by hyper-util's message text because it keeps that error type private. `hyper` is a direct dependency for the downcast; it is the same crate instance reqwest already compiles (#183).
* `resolve_url` returns a parsed `Url` and does the scheme check in three steps: parse, pick the absolute URL or the base join, check the scheme (#183).
* No changes to the request builder, transport, retry loop, or response types.

## Tests

* `CannedServer` fixture: answers every connection with fixed bytes and can close with a TCP reset, for protocol-level failure cases. Thirty new unit cases drive the buffered and streaming paths, sync and async, through malformed heads, broken bodies, corrupt encodings, resets, bad URLs and proxy replies (#183).
* Nine new equivalence cases run the same failures against `httpx.Client` and `rqx.Client`, including the proxy semantics; the two strict xfails for #114 and #88 are gone (#183).
* A stub-versus-runtime test for the whole exception tree (#183).

## Benchmarks & tooling

* Bench results are no longer stranded on the client. `collect` copies the run down with `scp` and uploads to S3 from your laptop; an upload failure is a warning and the local copy is complete. Three releases in a row had lost the upload leg to a missing CLI on the client (#184, closes #113).
* One script per step in `benchmarks/infra/scripts/` (`up`, `run`, `status`, `wait`, `collect`, `destroy`) and a `benchmarks/justfile` in front of them: `just benchmarks::setup <profile>` once, then `just benchmarks::release <version> <ref>`. Runs execute detached on the client, so closing your laptop does not end them, and `status` reports progress plus the b1 medians next to the last archive from any terminal (#184).
* `Pulumi.yaml` carries the shared defaults (region, instance type); `setup.sh` writes the per-operator stack config (profile, IP, key). The client no longer needs an IAM role (#184).
* `benchmarks/README.md` documents every bench: the question it answers, how to run it, how to read it, plus the local setup, the full sweep, methodology, confounders with measured run-to-run spread, and limitations (#184, closes #40).
* `benchmarks/compare_b1.py` prints b1 medians next to a baseline archive (#184).

## CI & packaging

* Dependabot: `urllib3` 2.7.0 (#178).

## Known issues

* #176 — building a `Client` costs ~35–50 ms on Linux (reqwest's default TLS backend loads the system CA store per client), and the module-level verbs build one per call. Unfixed.
* #168 — tail latency under load, unchanged.
* #167 — retries cover failures up to the response headers only, unchanged.
* Proxy env vars (`HTTP_PROXY` and friends) are honored by reqwest's defaults, as before; a proxy configured that way is classified the same as one passed to `HTTPTransport(proxy=...)`.

## Deferred

* #88 closed as not planned: rqx exceptions will not subclass httpx's. Every runtime route (conditional bases, `__instancecheck__`, rebinding httpx's names) makes `except` behave differently depending on whether an unrelated package is installed, or works for `isinstance` but not `except`. Migration is `from httpx import ConnectError` → `from rqx import ConnectError`; a migration guide is #37.
* `LocalProtocolError` and `CloseError` are not added: no rqx code path raises either.
* v0.3.0: `stream()` as an async context manager (#84), backpressure (#107), middleware (#147, #19), stub audit (#33). v1.0.0: LTO (#119) and the docs set (#116, #37, #36).
