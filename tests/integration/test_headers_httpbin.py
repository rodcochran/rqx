"""Headers taken off real responses (needs the local httpbin container)."""

import pytest

import rqx


def test_headers_from_response_is_case_insensitive(httpbin):
    """The headers attribute on a real response is also case-insensitive."""
    resp = rqx.Client().get(f"{httpbin}/get")
    assert resp.headers["Content-Type"] == resp.headers["content-type"]
    assert resp.headers["CONTENT-TYPE"] == resp.headers["Content-Type"]
    assert "Content-Type" in resp.headers
    assert "content-type" in resp.headers
    assert resp.headers.get("Content-Type") is not None
    assert resp.headers.get("missing-header") is None


@pytest.mark.asyncio
async def test_headers_from_async_response_is_case_insensitive(httpbin):
    client = rqx.AsyncClient()
    resp = await client.get(f"{httpbin}/get")
    assert resp.headers["Content-Type"] == resp.headers["content-type"]
    assert "Content-Type" in resp.headers
    assert "CONTENT-TYPE" in resp.headers


def test_response_headers_are_cached(httpbin):
    """`.headers` materializes once and returns the same object on repeat
    access — safe because a response's headers are read-only, and it matches
    httpx (`resp.headers is resp.headers`)."""
    resp = rqx.Client().get(f"{httpbin}/get")
    assert resp.headers is resp.headers
