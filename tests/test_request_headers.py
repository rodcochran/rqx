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
