"""Misusing a streamed response raises a specific class, not a bare RqxError
(https://github.com/rodcochran/rqx/issues/190)."""

import pytest

import rqx

URL_PATH = "/bigtext"


def test_stream_errors_are_rqx_and_runtime_errors_but_not_request_failures():
    assert issubclass(rqx.StreamError, rqx.RqxError)
    assert issubclass(rqx.StreamError, RuntimeError)
    assert not issubclass(rqx.StreamError, rqx.HTTPError)
    for cls in (rqx.StreamConsumed, rqx.StreamClosed, rqx.ResponseNotRead):
        assert issubclass(cls, rqx.StreamError)


# ----- sync -----


def test_second_iteration_raises_stream_consumed(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}{URL_PATH}") as resp:
        list(resp.iter_bytes())
        with pytest.raises(rqx.StreamConsumed):
            list(resp.iter_bytes())
        with pytest.raises(rqx.StreamConsumed):
            resp.read()


def test_iterating_a_body_read_into_memory_raises_stream_consumed(flaky_server):
    """Consume once: a body already buffered by read() is not iterated again."""
    with rqx.Client().stream("GET", f"{flaky_server}{URL_PATH}") as resp:
        resp.read()
        with pytest.raises(rqx.StreamConsumed):
            list(resp.iter_bytes())


def test_use_after_close_raises_stream_closed(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}{URL_PATH}") as resp:
        resp.close()
        with pytest.raises(rqx.StreamClosed):
            list(resp.iter_bytes())
        with pytest.raises(rqx.StreamClosed):
            resp.read()
        with pytest.raises(rqx.StreamClosed):
            _ = resp.content


def test_iterator_kept_past_close_raises_stream_closed(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}{URL_PATH}") as resp:
        chunks = resp.iter_text()
        next(chunks)
    with pytest.raises(rqx.StreamClosed):
        next(chunks)


def test_content_before_read_raises_response_not_read(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}{URL_PATH}") as resp:
        with pytest.raises(rqx.ResponseNotRead):
            _ = resp.content
        with pytest.raises(rqx.ResponseNotRead):
            _ = resp.text
        with pytest.raises(rqx.ResponseNotRead):
            resp.json()


# ----- async -----


@pytest.mark.asyncio
async def test_async_second_iteration_raises_stream_consumed(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}{URL_PATH}") as resp:
            async for _ in resp.aiter_bytes():
                pass
            with pytest.raises(rqx.StreamConsumed):
                resp.aiter_bytes()
            with pytest.raises(rqx.StreamConsumed):
                await resp.aread()


@pytest.mark.asyncio
async def test_async_iterating_a_body_read_into_memory_raises_stream_consumed(
    flaky_server,
):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}{URL_PATH}") as resp:
            await resp.aread()
            with pytest.raises(rqx.StreamConsumed):
                resp.aiter_bytes()


@pytest.mark.asyncio
async def test_async_use_after_close_raises_stream_closed(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}{URL_PATH}") as resp:
            await resp.aclose()
            with pytest.raises(rqx.StreamClosed):
                resp.aiter_bytes()
            with pytest.raises(rqx.StreamClosed):
                await resp.aread()
            with pytest.raises(rqx.StreamClosed):
                _ = resp.content


@pytest.mark.asyncio
async def test_async_iterator_kept_past_close_raises_stream_closed(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}{URL_PATH}") as resp:
            chunks = resp.aiter_text()
            await chunks.__anext__()
        with pytest.raises(rqx.StreamClosed):
            await chunks.__anext__()


@pytest.mark.asyncio
async def test_async_content_before_read_raises_response_not_read(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}{URL_PATH}") as resp:
            with pytest.raises(rqx.ResponseNotRead):
                _ = resp.content
