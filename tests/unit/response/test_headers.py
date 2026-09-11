"""Tests for the case-insensitive Headers class (Issue #14)."""

import pytest

import rqx


def test_headers_case_insensitive_getitem():
    """All casings of the same header name return the same value."""
    h = rqx.Headers({"Content-Type": "application/json"})
    assert h["Content-Type"] == "application/json"
    assert h["content-type"] == "application/json"
    assert h["CONTENT-TYPE"] == "application/json"
    assert h["Content-type"] == "application/json"


def test_headers_case_insensitive_contains():
    h = rqx.Headers({"Content-Type": "application/json"})
    assert "Content-Type" in h
    assert "content-type" in h
    assert "CONTENT-TYPE" in h
    assert "X-Missing" not in h


def test_headers_case_insensitive_get():
    h = rqx.Headers({"X-Custom-Header": "value"})
    assert h.get("X-Custom-Header") == "value"
    assert h.get("x-custom-header") == "value"
    assert h.get("X-CUSTOM-HEADER") == "value"
    assert h.get("missing") is None
    assert h.get("missing", "default") == "default"


def test_headers_setitem_replaces_existing_regardless_of_casing():
    h = rqx.Headers({"Content-Type": "text/plain"})
    h["content-type"] = "application/json"
    # Same key in any casing now returns the new value
    assert h["Content-Type"] == "application/json"
    assert h["content-type"] == "application/json"
    # Only one entry
    assert len(h) == 1


def test_headers_delitem_case_insensitive():
    h = rqx.Headers({"X-Foo": "bar"})
    del h["x-foo"]
    assert "X-Foo" not in h
    assert "x-foo" not in h


def test_headers_delitem_missing_raises():
    h = rqx.Headers({})
    with pytest.raises(KeyError):
        del h["X-Missing"]


def test_headers_getitem_missing_raises():
    h = rqx.Headers({})
    with pytest.raises(KeyError):
        h["X-Missing"]


def test_headers_iteration():
    h = rqx.Headers({"A": "1", "B": "2"})
    keys = list(h)
    assert set(keys) == {"a", "b"}  # http::HeaderMap normalizes to lowercase


def test_headers_len():
    h = rqx.Headers({"A": "1", "B": "2", "C": "3"})
    assert len(h) == 3


# --- entry cap: http's HeaderMap has 32768 slots at a 0.75 load factor, so
# it holds about 24576 entries. Past that must be a ValueError, not a panic.


def test_headers_constructor_past_the_cap_raises_value_error():
    with pytest.raises(ValueError, match="too many headers"):
        rqx.Headers({f"X-H-{i}": "v" for i in range(40_000)})


def test_headers_setitem_past_the_cap_raises_value_error():
    headers = rqx.Headers({f"X-H-{i}": "v" for i in range(20_000)})
    with pytest.raises(ValueError, match="too many headers"):
        for i in range(20_000, 40_000):
            headers[f"X-H-{i}"] = "v"


def test_headers_eq_against_an_oversized_mapping_is_false():
    headers = rqx.Headers({"X-Test": "1"})
    assert not (headers == {f"X-H-{i}": "v" for i in range(40_000)})
