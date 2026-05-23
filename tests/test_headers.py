"""Tests for the case-insensitive Headers class (Issue #14)."""

import pytest

import rqx

HTTPBIN_HOST = "http://localhost"


def test_headers_case_insensitive_getitem():
    """All casings of the same header name return the same value."""
    h = rqx.Headers({"Content-Type": "application/json"})
    assert h["Content-Type"] == "application/json"
    assert h["content-type"] == "application/json"
    assert h["CONTENT-TYPE"] == "application/json"
    assert h["Content-type"] == "application/json"


def test_headers_case_insensitive_contains():
    h = rqx.Headers({"Content-Type": "application/json"})
    assert "Content-Type" in h
    assert "content-type" in h
    assert "CONTENT-TYPE" in h
    assert "X-Missing" not in h


def test_headers_case_insensitive_get():
    h = rqx.Headers({"X-Custom-Header": "value"})
    assert h.get("X-Custom-Header") == "value"
    assert h.get("x-custom-header") == "value"
    assert h.get("X-CUSTOM-HEADER") == "value"
    assert h.get("missing") is None
    assert h.get("missing", "default") == "default"


def test_headers_setitem_replaces_existing_regardless_of_casing():
    h = rqx.Headers({"Content-Type": "text/plain"})
    h["content-type"] = "application/json"
    # Same key in any casing now returns the new value
    assert h["Content-Type"] == "application/json"
    assert h["content-type"] == "application/json"
    # Only one entry
    assert len(h) == 1


def test_headers_delitem_case_insensitive():
    h = rqx.Headers({"X-Foo": "bar"})
    del h["x-foo"]
    assert "X-Foo" not in h
    assert "x-foo" not in h


def test_headers_delitem_missing_raises():
    h = rqx.Headers({})
    with pytest.raises(KeyError):
        del h["X-Missing"]


def test_headers_getitem_missing_raises():
    h = rqx.Headers({})
    with pytest.raises(KeyError):
        h["X-Missing"]


def test_headers_iteration():
    h = rqx.Headers({"A": "1", "B": "2"})
    keys = list(h)
    assert set(keys) == {"a", "b"}  # http::HeaderMap normalizes to lowercase


def test_headers_len():
    h = rqx.Headers({"A": "1", "B": "2", "C": "3"})
    assert len(h) == 3


def test_headers_from_response_is_case_insensitive():
    """The headers attribute on a real response is also case-insensitive."""
    resp = rqx.Client().get(f"{HTTPBIN_HOST}/get")
    assert resp.headers["Content-Type"] == resp.headers["content-type"]
    assert resp.headers["CONTENT-TYPE"] == resp.headers["Content-Type"]
    assert "Content-Type" in resp.headers
    assert "content-type" in resp.headers
    assert resp.headers.get("Content-Type") is not None
    assert resp.headers.get("missing-header") is None


@pytest.mark.asyncio
async def test_headers_from_async_response_is_case_insensitive():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/get")
    assert resp.headers["Content-Type"] == resp.headers["content-type"]
    assert "Content-Type" in resp.headers
    assert "CONTENT-TYPE" in resp.headers


def test_response_headers_are_cached():
    """`.headers` materializes once and returns the same object on repeat
    access — safe because a response's headers are read-only, and it matches
    httpx (`resp.headers is resp.headers`)."""
    resp = rqx.Client().get(f"{HTTPBIN_HOST}/get")
    assert resp.headers is resp.headers
