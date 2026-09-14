"""stream() returns a context manager that sends on enter and closes on exit,
for both clients — the httpx shape (https://github.com/rodcochran/rqx/issues/84)."""

import pytest

import rqx

BODY = b'{"streamed": true}'


# ----- sync -----


def test_with_sends_on_enter_and_yields_response(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.status_code == 200
        assert resp.read() == BODY


def test_call_alone_does_not_send(flaky_server):
    """Before `with`, nothing has been sent: the object is a context, not a response."""
    ctx = rqx.Client().stream("GET", f"{flaky_server}/streamable")
    assert not hasattr(ctx, "status_code")
    assert not hasattr(ctx, "read")


def test_exit_releases_the_connection(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.is_closed is False
    assert resp.is_closed is True


def test_exception_inside_block_still_closes(flaky_server):
    with pytest.raises(RuntimeError):
        with rqx.Client().stream("GET", f"{flaky_server}/streamable") as resp:
            raise RuntimeError("boom")
    assert resp.is_closed is True


def test_context_is_single_use(flaky_server):
    ctx = rqx.Client().stream("GET", f"{flaky_server}/streamable")
    with ctx:
        pass
    with pytest.raises(rqx.RqxError, match="already"):
        with ctx:
            pass


def test_bad_url_raises_at_the_call():
    with pytest.raises(rqx.UnsupportedProtocol):
        rqx.Client().stream("GET", "example.com")


def test_send_failure_raises_at_enter():
    ctx = rqx.Client().stream("GET", "http://nonexistent.invalid/")
    with pytest.raises(rqx.ConnectError):
        with ctx:
            pass


def test_response_is_not_a_context_manager(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/streamable") as resp:
        assert not hasattr(resp, "__enter__")
        assert not hasattr(resp, "__exit__")


# ----- async -----


@pytest.mark.asyncio
async def test_async_with_sends_on_enter_and_yields_response(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/streamable") as resp:
            assert resp.status_code == 200
            assert await resp.aread() == BODY


@pytest.mark.asyncio
async def test_async_call_is_not_awaitable(flaky_server):
    async with rqx.AsyncClient() as client:
        ctx = client.stream("GET", f"{flaky_server}/streamable")
        assert not hasattr(ctx, "__await__")
        assert not hasattr(ctx, "status_code")


@pytest.mark.asyncio
async def test_async_exit_releases_the_connection(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/streamable") as resp:
            assert resp.is_closed is False
        assert resp.is_closed is True


@pytest.mark.asyncio
async def test_async_exception_inside_block_still_closes(flaky_server):
    async with rqx.AsyncClient() as client:
        with pytest.raises(RuntimeError):
            async with client.stream("GET", f"{flaky_server}/streamable") as resp:
                raise RuntimeError("boom")
        assert resp.is_closed is True


@pytest.mark.asyncio
async def test_async_context_is_single_use(flaky_server):
    async with rqx.AsyncClient() as client:
        ctx = client.stream("GET", f"{flaky_server}/streamable")
        async with ctx:
            pass
        with pytest.raises(rqx.RqxError, match="already"):
            async with ctx:
                pass


@pytest.mark.asyncio
async def test_async_bad_url_raises_at_the_call():
    async with rqx.AsyncClient() as client:
        with pytest.raises(rqx.UnsupportedProtocol):
            client.stream("GET", "example.com")


@pytest.mark.asyncio
async def test_async_send_failure_raises_at_enter():
    async with rqx.AsyncClient() as client:
        ctx = client.stream("GET", "http://nonexistent.invalid/")
        with pytest.raises(rqx.ConnectError):
            async with ctx:
                pass


@pytest.mark.asyncio
async def test_async_response_is_not_a_context_manager(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/streamable") as resp:
            assert not hasattr(resp, "__aenter__")
            assert not hasattr(resp, "__aexit__")


# ----- closing the response reaches its iterators -----

SHORT_BODY = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nhello"


def test_iterator_kept_past_the_block_raises_on_next(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/bigtext") as resp:
        chunks = resp.iter_bytes()
        first = next(chunks)
    assert first
    assert resp.is_closed is True
    with pytest.raises(rqx.RqxError, match="closed"):
        next(chunks)


def test_close_mid_iteration_stops_the_iterator(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/bigtext") as resp:
        lines = resp.iter_text()
        next(lines)
        resp.close()
        assert resp.is_closed is True
        with pytest.raises(rqx.RqxError, match="closed"):
            next(lines)


def test_stream_error_closes_the_response(canned_server):
    """A failed stream releases its connection without waiting for the iterator to be dropped."""
    with rqx.Client().stream("GET", canned_server(SHORT_BODY)) as resp:
        with pytest.raises(rqx.RemoteProtocolError):
            list(resp.iter_bytes())
        assert resp.is_closed is True


@pytest.mark.asyncio
async def test_async_iterator_kept_past_the_block_raises_on_next(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/bigtext") as resp:
            chunks = resp.aiter_bytes()
            first = await chunks.__anext__()
        assert first
        assert resp.is_closed is True
        with pytest.raises(rqx.RqxError, match="closed"):
            await chunks.__anext__()


@pytest.mark.asyncio
async def test_async_close_mid_iteration_stops_the_iterator(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/bigtext") as resp:
            text = resp.aiter_text()
            await text.__anext__()
            await resp.aclose()
            assert resp.is_closed is True
            with pytest.raises(rqx.RqxError, match="closed"):
                await text.__anext__()


@pytest.mark.asyncio
async def test_async_stream_error_closes_the_response(canned_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", canned_server(SHORT_BODY)) as resp:
            with pytest.raises(rqx.RemoteProtocolError):
                async for _ in resp.aiter_bytes():
                    pass
            assert resp.is_closed is True
