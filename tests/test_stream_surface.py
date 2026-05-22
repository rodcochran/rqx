"""Streaming response surface for issue #15 — sync PyStreamResponse.

Covers the read()/content/text/json buffering path, the ResponseNotRead-style
errors before a read, consume-once semantics, and that iter_text / text honor
the Content-Type charset (the streaming encoding-detection fix).

Uses the local `flaky_server` fixture for deterministic bodies:
  - /streamable -> application/json,  b'{"streamed": true}'
  - /latin1     -> text/plain; charset=iso-8859-1, "café" as 0xE9
"""

import pytest

import rqx

STREAMABLE_BODY = b'{"streamed": true}'


# ---------------------------------------------------------------------------
# read() — buffer the whole body
# ---------------------------------------------------------------------------


def test_read_returns_full_body(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.read() == STREAMABLE_BODY


def test_read_is_idempotent(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        first = resp.read()
        second = resp.read()
    assert first == second == STREAMABLE_BODY


# ---------------------------------------------------------------------------
# content / text / json after read()
# ---------------------------------------------------------------------------


def test_content_after_read(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        resp.read()
        assert resp.content == STREAMABLE_BODY


def test_content_is_same_object_as_read(flaky_server):
    # read() and .content share the single cached PyBytes materialization.
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        a = resp.read()
        b = resp.content
    assert a is b


def test_text_after_read(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        resp.read()
        assert resp.text == '{"streamed": true}'


def test_json_after_read(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        resp.read()
        assert resp.json() == {"streamed": True}


# ---------------------------------------------------------------------------
# Accessing the body before read() — ResponseNotRead parity
# ---------------------------------------------------------------------------


def test_content_before_read_raises(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        with pytest.raises(rqx.RqxError):
            _ = resp.content


def test_text_before_read_raises(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        with pytest.raises(rqx.RqxError):
            _ = resp.text


def test_json_before_read_raises(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        with pytest.raises(rqx.RqxError):
            resp.json()


# ---------------------------------------------------------------------------
# iter_text — reassembly + charset detection
# ---------------------------------------------------------------------------


def test_iter_text_reassembles_body(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        text = "".join(resp.iter_text())
    assert text == '{"streamed": true}'


def test_iter_text_honors_charset(flaky_server):
    # /latin1 advertises charset=iso-8859-1; é is the single byte 0xE9.
    # A UTF-8 decode would mangle it, so this proves resolved_encoding is wired.
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/latin1") as resp:
        text = "".join(resp.iter_text())
    assert text == "café"


def test_text_after_read_honors_charset(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/latin1") as resp:
        resp.read()
        assert resp.text == "café"


# ---------------------------------------------------------------------------
# Consume-once semantics — descriptive errors on re-consume
# ---------------------------------------------------------------------------


def test_iter_then_iter_raises(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        list(resp.iter_bytes())
        with pytest.raises(rqx.RqxError):
            list(resp.iter_bytes())


def test_read_then_iter_raises(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        resp.read()
        with pytest.raises(rqx.RqxError):  # "already read into memory"
            list(resp.iter_bytes())


def test_iter_then_read_raises(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        list(resp.iter_bytes())
        with pytest.raises(rqx.RqxError):  # "already consumed or closed"
            resp.read()


# ---------------------------------------------------------------------------
# json() on a non-JSON body after read()
# ---------------------------------------------------------------------------


def test_json_after_read_on_non_json_raises(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/latin1") as resp:
        resp.read()
        with pytest.raises(rqx.RqxError):
            resp.json()
