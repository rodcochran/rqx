# v0.4.1

```bash
pip install --upgrade rqx
```

Security fix. When following a redirect to a different origin, rqx sent the original request's `Authorization` header to the new host. A hand-set `Cookie` header followed every redirect as well. Both now behave as they do in httpx. Every earlier release that follows redirects is affected; upgrade if you use `follow_redirects=True` with credentials.

## Fixes

* **Drop `Authorization` on a redirect to another origin** (scheme, host or port), for basic auth, bearer tokens, a client-level `auth_bearer` and a hand-set `headers={"Authorization": ...}`. It is kept on same-origin hops and, as in httpx, on an `http` → `https` upgrade of the same host on default ports. rqx follows redirects itself, so reqwest's own stripping never ran (#215, closes #205).
* **Drop a hand-set `Cookie` header on every redirect.** The client's cookie jar supplies the cookies for each hop. Before, a hand-set `Cookie` reached the redirect target on any origin, and it also stopped the jar from adding that origin's own cookies (#215).

## Behavior changes

* A hand-set `Cookie` header no longer survives a **same-origin** redirect either, matching httpx. To keep a cookie across hops, let the server set it (the jar sends it on later hops) (#215).

## Additions

* `rqx.__version__`, set at compile time from the crate version, so it always matches the installed wheel. httpx has the same attribute.

## Performance

No bench run for this release. Requests that don't redirect run no new code; each redirect hop adds two header removals and an origin comparison. The charts and numbers in the README remain the 0.4.0 run ([`benchmarks/0.4.0/report.md`](https://github.com/rodcochran/rqx/blob/v0.4.1/benchmarks/0.4.0/report.md)).

## Internals

* `RqxClientUrl` removed. It forwarded every call to `UrlReference`, which `rqx.URL` now holds directly. `BaseUrl::new` takes the URL by value and the client's URL merge moves an absolute URL out instead of cloning it, so a `str` URL reaches reqwest without a copy (#211).

## Tests

* `tests/unit/redirect/test_redirect_credentials.py`: 43 cases across per-request, client-level and hand-set credentials, cross-host and cross-port targets, sync, async and `stream()` (#215).
* Equivalence cases against httpx for cross-origin `Authorization` and hand-set `Cookie` on same- and cross-origin redirects (#215).
* Rust unit tests for the origin rule, including the `http` → `https` upgrade exception and scheme-only changes the local servers can't reach (#215).
* Fixture additions: a `/redirect-to?url=` route and an `other_port_server` fixture (#215).

## Known issues

* #212 — `rqx.URL(..., query=..., params=...)` picks one of the two at random per process; httpx always uses `params`.
