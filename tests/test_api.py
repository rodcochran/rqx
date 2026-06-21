"""Module-level convenience functions (Issue #7).

Each ``rqx.<verb>`` spins up an ephemeral ``Client`` for one call. These tests
verify the dispatch works and that the relevant kwargs thread through to the
underlying client — they don't re-test HTTP semantics already covered by
``test_sync`` / ``test_async``.
"""

import pytest

import rqx


# ---------------------------------------------------------------------------
# Module-level functions exist and return responses
# ---------------------------------------------------------------------------


def test_get(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable")
    assert resp.status_code == 200
    assert resp.is_success is True


def test_head(flaky_server):
    # The test server doesn't implement HEAD (returns 501). The point of
    # these per-verb tests is to confirm the dispatch lands a request — not
    # to re-test server semantics — so any concrete status proves the
    # function wired through to the underlying client.
    resp = rqx.head(f"{flaky_server}/streamable")
    assert isinstance(resp.status_code, int)


def test_post(flaky_server):
    # do_POST on the flaky server returns 404 for unknown paths after
    # consuming any body — good enough to verify the dispatch works.
    resp = rqx.post(f"{flaky_server}/anything?request_id=api-post-test", json={"hi": 1})
    assert resp.status_code == 404


def test_put(flaky_server):
    resp = rqx.put(f"{flaky_server}/anything?request_id=api-put-test", json={"hi": 1})
    assert isinstance(resp.status_code, int)


def test_patch(flaky_server):
    resp = rqx.patch(f"{flaky_server}/anything?request_id=api-patch-test", json={"hi": 1})
    assert isinstance(resp.status_code, int)


def test_delete(flaky_server):
    resp = rqx.delete(f"{flaky_server}/streamable")
    assert isinstance(resp.status_code, int)


def test_options(flaky_server):
    resp = rqx.options(f"{flaky_server}/streamable")
    assert isinstance(resp.status_code, int)


def test_request_with_explicit_method(flaky_server):
    resp = rqx.request("GET", f"{flaky_server}/streamable")
    assert resp.status_code == 200


# ---------------------------------------------------------------------------
# Kwargs thread through
# ---------------------------------------------------------------------------


def test_params_threaded_through(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params={"foo": "bar"})
    assert resp.status_code == 200
    assert "foo=bar" in resp.url


def test_params_accept_httpx_style_scalars(flaky_server):
    resp = rqx.get(
        f"{flaky_server}/streamable",
        params={"page": 1, "active": True, "ratio": 0.5, "empty": None},
    )

    assert resp.status_code == 200
    assert "page=1" in resp.url
    assert "active=true" in resp.url
    assert "ratio=0.5" in resp.url
    assert "empty=" not in resp.url


def test_headers_threaded_through(flaky_server):
    """A custom header round-trips — we don't have an echo endpoint, but we
    can at least confirm the call doesn't fail with the kwarg present."""
    resp = rqx.get(f"{flaky_server}/streamable", headers={"X-Test": "yes"})
    assert resp.status_code == 200


def test_timeout_threaded_through(flaky_server):
    """A per-call timeout shorter than the server's sleep raises ReadTimeout."""
    with pytest.raises(rqx.ReadTimeout):
        rqx.get(f"{flaky_server}/sleep/2", timeout=0.5)


def test_follow_redirects_disabled_by_default(flaky_server):
    resp = rqx.get(f"{flaky_server}/redirect-once")
    assert resp.status_code == 302  # not followed


def test_follow_redirects_enabled(flaky_server):
    resp = rqx.get(f"{flaky_server}/redirect-once", follow_redirects=True)
    assert resp.status_code == 200
    assert resp.url.endswith("/streamable")


# ---------------------------------------------------------------------------
# stream() context manager
# ---------------------------------------------------------------------------


def test_stream_yields_response(flaky_server):
    """``rqx.stream`` works as a contextmanager and exposes iter_bytes."""
    with rqx.stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.status_code == 200
        body = b"".join(resp.iter_bytes())
        assert body == b'{"streamed": true}'


def test_stream_with_follow_redirects(flaky_server):
    with rqx.stream("GET", f"{flaky_server}/redirect-once", follow_redirects=True) as resp:
        assert resp.status_code == 200
        assert resp.url.endswith("/streamable")


# ---------------------------------------------------------------------------
# Symbols are actually exposed
# ---------------------------------------------------------------------------


def test_all_verbs_in_public_api():
    """All eight verbs plus request and stream are on the rqx module."""
    for name in ("request", "stream", "get", "post", "put", "patch", "delete", "head", "options"):
        assert hasattr(rqx, name), f"rqx.{name} missing"
    # And they're in __all__ (so `from rqx import *` picks them up).
    for name in ("request", "stream", "get", "post", "put", "patch", "delete", "head", "options"):
        assert name in rqx.__all__
