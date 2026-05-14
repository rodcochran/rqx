"""Tests for resp.encoding getter/setter (Issue #10).

Resolution order for the encoding used by `.text`:
  1. Explicit `resp.encoding = "..."` override
  2. `charset=` parameter on the Content-Type header
  3. UTF-8 fallback
"""

import rqx


def test_encoding_defaults_to_utf8_when_no_charset_header(flaky_server):
    """A response with no charset in Content-Type reports utf-8."""
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/streamable")  # application/json, no charset
    assert resp.encoding == "utf-8"


def test_encoding_picks_up_charset_from_header(flaky_server):
    """Content-Type: text/plain; charset=iso-8859-1 → resp.encoding == iso-8859-1."""
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/latin1")
    # encoding_rs canonicalizes "iso-8859-1" to "windows-1252" (same family),
    # so we accept either spelling — both decode the body correctly.
    assert resp.encoding in {"iso-8859-1", "windows-1252"}


def test_text_decodes_using_header_charset(flaky_server):
    """The /latin1 body is 'café' encoded as latin-1; without override, it
    decodes correctly because the header advertises the charset."""
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/latin1")
    assert resp.text == "café"


def test_encoding_override_changes_decode(flaky_server):
    """Setting resp.encoding to a wrong encoding produces a different .text result."""
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/latin1")

    # Same body, but now decoded as utf-8. The 0xE9 byte is not valid utf-8 on
    # its own — encoding_rs replaces it with U+FFFD rather than raising.
    resp.encoding = "utf-8"
    assert resp.encoding == "utf-8"
    assert "café" not in resp.text


def test_encoding_setter_persists(flaky_server):
    """The setter sticks: a subsequent get returns the set value, not the header value."""
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/streamable")
    assert resp.encoding == "utf-8"
    resp.encoding = "ascii"
    assert resp.encoding == "ascii"


def test_encoding_unknown_label_falls_back_to_utf8_for_decoding(flaky_server):
    """Setting a label encoding_rs doesn't recognize falls back to UTF-8 silently
    when decoding (so .text never raises). The getter still echoes the label
    the user set, mirroring httpx."""
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/streamable")
    resp.encoding = "not-a-real-encoding"
    # Getter echoes what the user set.
    assert resp.encoding == "not-a-real-encoding"
    # Decoder falls back to utf-8 internally — body is ASCII, so this works.
    assert resp.text == '{"streamed": true}'
