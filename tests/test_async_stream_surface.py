"""Async streaming response surface for issue #15 — PyAsyncStreamResponse.

Mirrors test_stream_surface.py for the async path: aread() buffers in place
(Arc<Mutex> store-back), and the *sync* content/text/json accessors work
afterward — same httpx shape, where only read is async (no atext/ajson).
"""

import pytest

import rqx

STREAMABLE_BODY = b'{"streamed": true}'


@pytest.mark.asyncio
async def test_aread_returns_full_body(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        assert await resp.aread() == STREAMABLE_BODY


@pytest.mark.asyncio
async def test_aread_is_idempotent(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        first = await resp.aread()
        second = await resp.aread()
    assert first == second == STREAMABLE_BODY


@pytest.mark.asyncio
async def test_content_text_json_after_aread(flaky_server):
    # The httpx parity payoff: await aread(), then the *sync* accessors work.
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        await resp.aread()
        assert resp.content == STREAMABLE_BODY
        assert resp.text == '{"streamed": true}'
        assert resp.json() == {"streamed": True}


@pytest.mark.asyncio
async def test_accessors_before_aread_raise(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        with pytest.raises(rqx.RqxError):
            _ = resp.content
        with pytest.raises(rqx.RqxError):
            _ = resp.text
        with pytest.raises(rqx.RqxError):
            resp.json()


@pytest.mark.asyncio
async def test_text_after_aread_honors_charset(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/latin1")
        await resp.aread()
        assert resp.text == "café"


@pytest.mark.asyncio
async def test_fresh_stream_not_consumed_not_closed(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        assert resp.is_consumed is False
        assert resp.is_closed is False


@pytest.mark.asyncio
async def test_aread_consumes_but_does_not_close(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        await resp.aread()
        assert resp.is_consumed is True
        assert resp.is_closed is False  # buffered — still readable


@pytest.mark.asyncio
async def test_streaming_consumes_and_closes(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        async for _ in resp.aiter_bytes():
            pass
        assert resp.is_consumed is True
        assert resp.is_closed is True


@pytest.mark.asyncio
async def test_aclose_then_iter_raises(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        await resp.aclose()
        assert resp.is_closed is True
        with pytest.raises(rqx.RqxError):
            async for _ in resp.aiter_bytes():
                pass


# ---------------------------------------------------------------------------
# aiter_text / aiter_lines
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_aiter_text_reassembles_body(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/streamable")
        parts = [chunk async for chunk in resp.aiter_text()]
    assert "".join(parts) == '{"streamed": true}'


@pytest.mark.asyncio
async def test_aiter_text_honors_charset(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/latin1")
        parts = [chunk async for chunk in resp.aiter_text()]
    assert "".join(parts) == "café"


@pytest.mark.asyncio
async def test_aiter_text_reassembles_multibyte_across_chunks(flaky_server):
    # Async analog of the sync /bigtext test: the decoder must hold partial
    # multibyte chars across __anext__ calls.
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/bigtext")
        text = "".join([chunk async for chunk in resp.aiter_text()])
    assert text == "aé€🙂" * 100_000
    assert "�" not in text


@pytest.mark.asyncio
async def test_aiter_lines_splits_and_strips_terminators(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{flaky_server}/lines")
        lines = [line async for line in resp.aiter_lines()]
    assert lines == ["first", "second", "third"]
