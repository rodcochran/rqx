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


# ---------------------------------------------------------------------------
# Cross-chunk reassembly — multibyte chars split across network chunks
# ---------------------------------------------------------------------------


def test_bigtext_spans_multiple_chunks(flaky_server):
    # Guards the reassembly test below: if the ~1 MB body arrived in a single
    # chunk, that test would be vacuous. Over a real socket it splits into many.
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/bigtext") as resp:
        n_chunks = len(list(resp.iter_bytes()))
    assert n_chunks > 1


def test_iter_text_reassembles_multibyte_across_chunks(flaky_server):
    # The decoder must hold a partial multibyte char across __next__ calls.
    # With ~1 MB of 1/2/3/4-byte chars, a chunk boundary almost certainly lands
    # mid-character; broken reassembly would yield U+FFFD or the wrong length.
    expected = "aé€🙂" * 100_000
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/bigtext") as resp:
        text = "".join(resp.iter_text())
    assert text == expected
    assert "�" not in text


# ---------------------------------------------------------------------------
# State machine — is_consumed / is_closed / close
# ---------------------------------------------------------------------------


def test_fresh_stream_not_consumed_not_closed(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.is_consumed is False
        assert resp.is_closed is False


def test_read_consumes_but_does_not_close(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        resp.read()
        assert resp.is_consumed is True
        assert resp.is_closed is False  # buffered — still readable


def test_streaming_consumes_and_closes(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        list(resp.iter_bytes())
        assert resp.is_consumed is True
        assert resp.is_closed is True  # streamed off, nothing retained


def test_close_then_iter_raises(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        resp.close()
        assert resp.is_closed is True
        with pytest.raises(rqx.RqxError):
            list(resp.iter_bytes())


def test_close_after_read_drops_buffer(flaky_server):
    # close() sets body = None, which drops the buffer — the deliberate
    # "close drops everything" choice. .content then errors.
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        resp.read()
        assert resp.content  # readable while buffered
        resp.close()
        assert resp.is_closed is True
        with pytest.raises(rqx.RqxError):
            _ = resp.content


# ---------------------------------------------------------------------------
# iter_lines — end-to-end wiring (cross-chunk edge cases live in Rust unit tests)
# ---------------------------------------------------------------------------


def test_iter_lines_splits_and_strips_terminators(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/lines") as resp:
        lines = list(resp.iter_lines())
    assert lines == ["first", "second", "third"]


def test_stream_headers_are_cached(flaky_server):
    # Same cached-headers behavior as the buffered response.
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.headers is resp.headers
