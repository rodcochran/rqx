"""`params=` scalar coercion (https://github.com/rodcochran/rqx/issues/115).

httpx accepts str / int / float / bool / None values and stringifies them
its way: bools are lowercase, None drops the key, floats keep Python's
`str()` formatting. Every test reads the query back off `resp.url`.
"""

from types import MappingProxyType

import pytest
import rqx


def _query(url: str) -> str:
    return url.split("?", 1)[1] if "?" in url else ""


def test_int_value_is_stringified(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params={"page": 1})
    assert resp.status_code == 200
    assert _query(resp.url) == "page=1"


def test_bool_values_use_httpx_lowercase(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params={"on": True, "off": False})
    assert _query(resp.url) == "on=true&off=false"


def test_float_value_is_stringified(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params={"ratio": 0.5})
    assert _query(resp.url) == "ratio=0.5"


def test_float_keeps_python_formatting(flaky_server):
    """`str(1e16)` is `1e+16` in Python; Rust would print all the digits.
    httpx sends Python's form, so rqx must too. The `+` is percent-encoded."""
    resp = rqx.get(f"{flaky_server}/streamable", params={"big": 1e16})
    assert _query(resp.url) == "big=1e%2B16"


def test_int_beyond_i64_is_stringified(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params={"n": 2**64})
    assert _query(resp.url) == "n=18446744073709551616"


def test_none_value_drops_the_key(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params={"keep": "x", "drop": None})
    assert _query(resp.url) == "keep=x"


def test_all_none_values_send_no_query(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params={"drop": None})
    assert resp.status_code == 200
    assert "?" not in resp.url


def test_str_values_pass_through_encoded(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params={"q": "a b&c"})
    assert _query(resp.url) == "q=a+b%26c"


def test_mapping_order_is_preserved(flaky_server):
    """Dict insertion order is the wire order, like httpx."""
    ordered = {"e": 1, "d": 2, "c": 3, "b": 4, "a": 5}
    resp = rqx.get(f"{flaky_server}/streamable", params=ordered)
    assert _query(resp.url) == "e=1&d=2&c=3&b=4&a=5"


def test_non_dict_mapping_is_accepted(flaky_server):
    resp = rqx.get(f"{flaky_server}/streamable", params=MappingProxyType({"page": 2}))
    assert _query(resp.url) == "page=2"


def test_non_mapping_raises_type_error(flaky_server):
    with pytest.raises(TypeError, match="mapping"):
        rqx.get(f"{flaky_server}/streamable", params="page=1")  # ty: ignore[invalid-argument-type]


def test_unsupported_value_type_raises_type_error(flaky_server):
    """Sequence values (httpx multi-value params) are deliberately out of
    scope for now; they must fail loudly rather than be stringified."""
    with pytest.raises(TypeError, match="str, int, float, bool, or None"):
        rqx.get(f"{flaky_server}/streamable", params={"ids": [1, 2]})  # ty: ignore[invalid-argument-type]


def test_non_str_key_raises_type_error(flaky_server):
    with pytest.raises(TypeError, match="keys must be str"):
        rqx.get(f"{flaky_server}/streamable", params={1: "a"})  # ty: ignore[invalid-argument-type]


def test_non_str_key_with_none_value_still_raises(flaky_server):
    """Key validation must not be skipped by the None short-circuit."""
    with pytest.raises(TypeError, match="keys must be str"):
        rqx.get(f"{flaky_server}/streamable", params={1: None})  # ty: ignore[invalid-argument-type]


def test_client_verbs_share_the_coercion(flaky_server):
    client = rqx.Client(base_url=flaky_server)
    resp = client.get("/streamable", params={"page": 1, "active": True})
    assert _query(resp.url) == "page=1&active=true"


@pytest.mark.asyncio
async def test_async_client_shares_the_coercion(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.get(
            f"{flaky_server}/streamable",
            params={"page": 1, "active": True, "ratio": 0.5, "empty": None},
        )
    assert _query(resp.url) == "page=1&active=true&ratio=0.5"
