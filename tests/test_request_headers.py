"""`headers=` on the request path (https://github.com/rodcochran/rqx/issues/117).

Invalid names, values, and methods must surface as ValueError, never a
panic. Valid headers go straight onto the wire; `/echo-headers` returns
what the server received as [name, value] pairs, duplicates included.
"""

from types import MappingProxyType

import pytest

import rqx


def _sent(resp) -> dict[str, list[str]]:
    """Group echoed [name, value] pairs by lowercased name."""
    out: dict[str, list[str]] = {}
    for name, value in resp.json():
        out.setdefault(name.lower(), []).append(value)
    return out


# --- invalid input is a ValueError, not a panic -----------------------------


def test_invalid_header_name_raises_value_error(flaky_server):
    with pytest.raises(ValueError, match='invalid header name "bad name"'):
        rqx.get(f"{flaky_server}/echo-headers", headers={"bad name": "x"})


def test_invalid_header_value_raises_value_error(flaky_server):
    with pytest.raises(ValueError, match="invalid header value"):
        rqx.get(f"{flaky_server}/echo-headers", headers={"X-Test": "line1\nline2"})


def test_invalid_method_raises_value_error(flaky_server):
    with pytest.raises(ValueError, match='invalid method "GE T"'):
        rqx.request("GE T", f"{flaky_server}/echo-headers")


def test_non_mapping_headers_raises_type_error(flaky_server):
    with pytest.raises(TypeError, match="mapping"):
        rqx.get(f"{flaky_server}/echo-headers", headers="X-Test: 1")


def test_non_str_header_value_raises_type_error(flaky_server):
    with pytest.raises(TypeError, match="header values must be str"):
        rqx.get(f"{flaky_server}/echo-headers", headers={"X-Test": 1})


def test_non_str_header_name_raises_type_error(flaky_server):
    with pytest.raises(TypeError, match="header names must be str"):
        rqx.get(f"{flaky_server}/echo-headers", headers={1: "x"})


# --- valid input reaches the wire -------------------------------------------


def test_headers_reach_the_server(flaky_server):
    resp = rqx.get(
        f"{flaky_server}/echo-headers", headers={"X-Test": "yes", "X-Other": "2"}
    )
    sent = _sent(resp)
    assert sent["x-test"] == ["yes"]
    assert sent["x-other"] == ["2"]


def test_case_variant_names_are_both_sent(flaky_server):
    """Two dict keys that differ only by case are two header lines, like httpx."""
    resp = rqx.get(
        f"{flaky_server}/echo-headers", headers={"X-Test": "a", "x-test": "b"}
    )
    assert _sent(resp)["x-test"] == ["a", "b"]


def test_non_dict_mapping_is_accepted(flaky_server):
    resp = rqx.get(
        f"{flaky_server}/echo-headers", headers=MappingProxyType({"X-Test": "m"})
    )
    assert _sent(resp)["x-test"] == ["m"]


def test_headers_instance_is_accepted(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-headers", headers=rqx.Headers({"X-Test": "h"}))
    assert _sent(resp)["x-test"] == ["h"]


def test_client_verbs_share_the_path(flaky_server):
    client = rqx.Client(base_url=flaky_server)
    resp = client.get("/echo-headers", headers={"X-Test": "c"})
    assert _sent(resp)["x-test"] == ["c"]
    with pytest.raises(ValueError, match="invalid header name"):
        client.get("/echo-headers", headers={"bad name": "x"})


@pytest.mark.asyncio
async def test_async_client_shares_the_path(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.get(f"{flaky_server}/echo-headers", headers={"X-Test": "a"})
        assert _sent(resp)["x-test"] == ["a"]
        with pytest.raises(ValueError, match="invalid header name"):
            await client.get(f"{flaky_server}/echo-headers", headers={"bad name": "x"})


# --- edge cases -------------------------------------------------------------


def test_empty_headers_mapping_is_fine(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-headers", headers={})
    assert resp.status_code == 200
    assert "x-test" not in _sent(resp)


def test_empty_header_value_is_sent(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-headers", headers={"X-Test": ""})
    assert _sent(resp)["x-test"] == [""]


def test_empty_header_name_raises_value_error(flaky_server):
    with pytest.raises(ValueError, match='invalid header name ""'):
        rqx.get(f"{flaky_server}/echo-headers", headers={"": "x"})


@pytest.mark.parametrize("name", ["X-Test:", "X\tTest", "(x)", "X-Tëst", "X/Test"])
def test_non_token_header_names_raise_value_error(flaky_server, name):
    with pytest.raises(ValueError, match="invalid header name"):
        rqx.get(f"{flaky_server}/echo-headers", headers={name: "x"})


@pytest.mark.parametrize("value", ["a\rb", "a\x00b", "a\x7fb", "a\nb"])
def test_control_chars_in_header_values_raise_value_error(flaky_server, value):
    with pytest.raises(ValueError, match="invalid header value"):
        rqx.get(f"{flaky_server}/echo-headers", headers={"X-Test": value})


def test_inner_whitespace_in_header_value_is_preserved(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-headers", headers={"X-Test": "a \t b"})
    assert _sent(resp)["x-test"] == ["a \t b"]


def test_long_header_value_round_trips(flaky_server):
    value = "v" * 8000
    resp = rqx.get(f"{flaky_server}/echo-headers", headers={"X-Test": value})
    assert _sent(resp)["x-test"] == [value]


def test_bytes_header_value_raises_type_error(flaky_server):
    """httpx accepts bytes values; rqx is str-only for now, and says so."""
    with pytest.raises(TypeError, match="header values must be str, got bytes"):
        rqx.get(f"{flaky_server}/echo-headers", headers={"X-Test": b"x"})


def test_none_header_value_raises_type_error(flaky_server):
    with pytest.raises(TypeError, match="header values must be str, got NoneType"):
        rqx.get(f"{flaky_server}/echo-headers", headers={"X-Test": None})


def test_request_header_overrides_client_default(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-headers", headers={"User-Agent": "custom/1"})
    assert _sent(resp)["user-agent"] == ["custom/1"]


def test_mapping_whose_items_raises_propagates_that_error(flaky_server):
    """A real exception from items() must not be masked as 'not a mapping'."""

    class Broken:
        def items(self):
            raise RuntimeError("boom")

    with pytest.raises(RuntimeError, match="boom"):
        rqx.get(f"{flaky_server}/echo-headers", headers=Broken())


def test_mapping_yielding_non_pairs_raises_cleanly(flaky_server):
    class Triples:
        def items(self):
            return [("X-Test", "a", "extra")]

    with pytest.raises((TypeError, ValueError)):
        rqx.get(f"{flaky_server}/echo-headers", headers=Triples())


def test_non_callable_items_attribute_raises_type_error(flaky_server):
    class NotCallable:
        items = 42

    with pytest.raises(TypeError):
        rqx.get(f"{flaky_server}/echo-headers", headers=NotCallable())


def test_more_headers_than_the_map_allows_raises_value_error(flaky_server):
    """http's HeaderMap caps at 32768 entries; past that must be a ValueError,
    not a panic."""
    headers = {f"X-H-{i}": "v" for i in range(40_000)}
    with pytest.raises(ValueError, match="too many headers"):
        rqx.get(f"{flaky_server}/echo-headers", headers=headers)


def test_mapping_with_a_lying_len_still_works(flaky_server):
    """`__len__` is only a capacity hint; an absurd one must not error."""

    class Liar:
        def __len__(self):
            return 1 << 40

        def items(self):
            return [("X-Test", "liar")]

    resp = rqx.get(f"{flaky_server}/echo-headers", headers=Liar())
    assert _sent(resp)["x-test"] == ["liar"]


# --- method edge cases ------------------------------------------------------


def test_lowercase_method_is_uppercased_like_httpx(flaky_server):
    resp = rqx.request("get", f"{flaky_server}/echo-headers")
    assert resp.status_code == 200


def test_custom_method_token_is_sent_without_client_error(flaky_server):
    """The fixture doesn't implement PURGE, so it answers 501. The point is
    that the client accepted a valid non-standard token."""
    resp = rqx.request("PURGE", f"{flaky_server}/echo-headers")
    assert resp.status_code == 501


@pytest.mark.parametrize("method", ["", "GÉT", "GE\tT", "GET/"])
def test_non_token_methods_raise_value_error(flaky_server, method):
    with pytest.raises(ValueError, match="invalid method"):
        rqx.request(method, f"{flaky_server}/echo-headers")
