"""Response-surface behaviors with httpx's names now have httpx's results
(https://github.com/rodcochran/rqx/issues/189)."""

import json
from datetime import timedelta

import pytest

import rqx

NOT_JSON = b'{"a": 1,\n "b": x}'


def _plain(body: bytes, content_type: str = "application/json") -> bytes:
    head = f"HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {len(body)}\r\n\r\n"
    return head.encode() + body


# ----- raise_for_status() returns the response -----


def test_raise_for_status_returns_the_response(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable")
    assert resp.raise_for_status() is resp
    assert resp.raise_for_status().json() == {"streamed": True}


def test_stream_raise_for_status_returns_the_response(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.raise_for_status() is resp


@pytest.mark.asyncio
async def test_async_stream_raise_for_status_returns_the_response(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/streamable") as resp:
            assert resp.raise_for_status() is resp


# ----- HTTPStatusError carries the response -----


def test_status_error_carries_the_response(flaky_server):
    resp = rqx.Client().delete(f"{flaky_server}/no-such-route")
    with pytest.raises(rqx.HTTPStatusError) as caught:
        resp.raise_for_status()
    assert caught.value.response is resp
    assert caught.value.response.status_code == 404


def test_stream_status_error_carries_the_response(flaky_server):
    with rqx.Client().stream("DELETE", f"{flaky_server}/no-such-route") as resp:
        with pytest.raises(rqx.HTTPStatusError) as caught:
            resp.raise_for_status()
        assert caught.value.response is resp


@pytest.mark.asyncio
async def test_async_stream_status_error_carries_the_response(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("DELETE", f"{flaky_server}/no-such-route") as resp:
            with pytest.raises(rqx.HTTPStatusError) as caught:
                resp.raise_for_status()
            assert caught.value.response is resp


# ----- elapsed is a timedelta measured to the headers -----


def test_elapsed_is_a_timedelta(flaky_server):
    resp = rqx.get(f"{flaky_server}/sleep/0.2")
    assert isinstance(resp.elapsed, timedelta)
    assert resp.elapsed >= timedelta(seconds=0.2)


def test_buffered_elapsed_stops_at_the_headers(flaky_server):
    """The body arrives 0.5 s after the headers; elapsed doesn't include that wait."""
    resp = rqx.get(f"{flaky_server}/slow-body/0.5")
    assert resp.json() == {"slow": True}
    assert resp.elapsed < timedelta(seconds=0.4)


@pytest.mark.asyncio
async def test_async_buffered_elapsed_is_a_timedelta_to_the_headers(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.get(f"{flaky_server}/slow-body/0.5")
    assert isinstance(resp.elapsed, timedelta)
    assert resp.elapsed < timedelta(seconds=0.4)


def test_stream_elapsed_is_a_timedelta(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/streamable") as resp:
        assert isinstance(resp.elapsed, timedelta)


# ----- json() raises rqx.JSONDecodeError, which is also json.JSONDecodeError -----


def test_json_decode_error_class_has_both_bases():
    assert issubclass(rqx.JSONDecodeError, rqx.RqxError)
    assert issubclass(rqx.JSONDecodeError, json.JSONDecodeError)
    assert issubclass(rqx.JSONDecodeError, ValueError)
    assert not issubclass(rqx.JSONDecodeError, rqx.HTTPError)


def test_invalid_json_raises_stdlib_catchable_error(canned_server):
    resp = rqx.get(canned_server(_plain(NOT_JSON)))
    with pytest.raises(json.JSONDecodeError) as caught:
        resp.json()
    err = caught.value
    assert isinstance(err, rqx.RqxError)
    assert err.doc == NOT_JSON.decode()
    # Same position as the stdlib parser reports for the same document.
    with pytest.raises(json.JSONDecodeError) as stdlib:
        json.loads(NOT_JSON)
    assert (err.pos, err.lineno, err.colno) == (
        stdlib.value.pos,
        stdlib.value.lineno,
        stdlib.value.colno,
    )


def test_invalid_json_message_names_status_and_content_type(canned_server):
    resp = rqx.get(canned_server(_plain(b"<html>", "text/html")))
    with pytest.raises(rqx.JSONDecodeError, match="HTTP 200.*text/html"):
        resp.json()


def test_stream_invalid_json_raises_json_decode_error(canned_server):
    with rqx.Client().stream("GET", canned_server(_plain(NOT_JSON))) as resp:
        resp.read()
        with pytest.raises(json.JSONDecodeError):
            resp.json()


@pytest.mark.asyncio
async def test_async_stream_invalid_json_raises_json_decode_error(canned_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", canned_server(_plain(NOT_JSON))) as resp:
            await resp.aread()
            with pytest.raises(json.JSONDecodeError):
                resp.json()
