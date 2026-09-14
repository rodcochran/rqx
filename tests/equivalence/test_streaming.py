"""stream() is a context manager in both clients; the body arrives through the iterators."""

import pytest


def test_stream_context_yields_iterable_body(lib, flaky_server):
    with lib.client().stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.status_code == 200
        assert b"".join(resp.iter_bytes()) == b'{"streamed": true}'


def test_stream_context_closes_on_exit(lib, flaky_server):
    with lib.client().stream("GET", f"{flaky_server}/streamable") as resp:
        pass
    assert resp.is_closed


@pytest.mark.asyncio
async def test_async_stream_context_yields_iterable_body(lib, flaky_server):
    async with lib.async_client() as client:
        async with client.stream("GET", f"{flaky_server}/streamable") as resp:
            assert resp.status_code == 200
            chunks = [chunk async for chunk in resp.aiter_bytes()]
    assert b"".join(chunks) == b'{"streamed": true}'


@pytest.mark.asyncio
async def test_async_stream_context_closes_on_exit(lib, flaky_server):
    async with lib.async_client() as client:
        async with client.stream("GET", f"{flaky_server}/streamable") as resp:
            pass
        assert resp.is_closed
