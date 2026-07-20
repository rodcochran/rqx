"""Tests for client.stream() with follow_redirects=True (Issue #3)."""

import pytest

import rqx


# ----- sync -----


def test_stream_follow_redirects_completes_chain(flaky_server):
    """/redirect-once → 302 → /streamable. Stream should yield the final body."""
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/redirect-once", follow_redirects=True) as resp:
        chunks = list(resp.iter_bytes())
    body = b"".join(chunks)
    assert body == b'{"streamed": true}'


def test_stream_follow_redirects_does_not_panic(flaky_server):
    """Regression for the original todo!() panic on this code path."""
    client = rqx.Client()
    # Just exercise the path; assertion is "no exception escapes."
    with client.stream("GET", f"{flaky_server}/redirect-once", follow_redirects=True) as resp:
        for _ in resp.iter_bytes():
            pass


def test_stream_follow_redirects_too_many_redirects(flaky_server):
    """Loop exceeds max_redirects → TooManyRedirects."""
    client = rqx.Client(max_redirects=3)
    with pytest.raises(rqx.TooManyRedirects):
        client.stream(
            "GET", f"{flaky_server}/redirect-loop", follow_redirects=True
        ).__enter__()


def test_stream_raise_on_redirect_false_returns_3xx(flaky_server):
    """raise_on_redirect=False returns the last 3xx as a stream rather than raising."""
    retries = rqx.Retry(raise_on_redirect=False)
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, max_redirects=3)
    with client.stream(
        "GET", f"{flaky_server}/redirect-loop", follow_redirects=True
    ) as resp:
        assert 300 <= resp.status_code < 400


def test_stream_no_follow_still_works(flaky_server):
    """Sanity check: follow_redirects=False against a redirect endpoint returns the 3xx."""
    client = rqx.Client()
    with client.stream(
        "GET", f"{flaky_server}/redirect-once", follow_redirects=False
    ) as resp:
        assert 300 <= resp.status_code < 400


# ----- async -----


@pytest.mark.asyncio
async def test_stream_follow_redirects_completes_chain_async(flaky_server):
    client = rqx.AsyncClient()
    async with await client.stream(
        "GET", f"{flaky_server}/redirect-once", follow_redirects=True
    ) as resp:
        chunks = []
        async for chunk in resp.aiter_bytes():
            chunks.append(chunk)
    body = b"".join(chunks)
    assert body == b'{"streamed": true}'


@pytest.mark.asyncio
async def test_stream_follow_redirects_too_many_redirects_async(flaky_server):
    client = rqx.AsyncClient(max_redirects=3)
    with pytest.raises(rqx.TooManyRedirects):
        await client.stream(
            "GET", f"{flaky_server}/redirect-loop", follow_redirects=True
        )


@pytest.mark.asyncio
async def test_stream_raise_on_redirect_false_returns_3xx_async(flaky_server):
    retries = rqx.Retry(raise_on_redirect=False)
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(transport=transport, max_redirects=3)
    async with await client.stream(
        "GET", f"{flaky_server}/redirect-loop", follow_redirects=True
    ) as resp:
        assert 300 <= resp.status_code < 400
