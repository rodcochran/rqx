"""`json=` body encoding (https://github.com/rodcochran/rqx/issues/118).

Target is what httpx sends: stdlib `json.dumps(obj, ensure_ascii=False,
separators=(",", ":"), allow_nan=False)`. Panics and silent `null`s are
bugs; unsupported input must raise the same exception type as stdlib.
`/echo-body` returns the exact bytes that went on the wire.
"""

import json
from datetime import datetime, timezone
from enum import IntEnum

import pytest

import rqx


def _wire(flaky_server, payload) -> str:
    resp = rqx.post(f"{flaky_server}/echo-body", json=payload)
    assert resp.status_code == 200
    return resp.json()["body"]


def _stdlib(payload) -> str:
    return json.dumps(
        payload, ensure_ascii=False, separators=(",", ":"), allow_nan=False
    )


# --- byte-for-byte parity with stdlib ----------------------------------------


def test_dict_insertion_order_is_preserved(flaky_server):
    payload = {"b": 1, "a": 2, "c": 3}
    assert _wire(flaky_server, payload) == _stdlib(payload)


def test_tuple_encodes_as_array(flaky_server):
    payload = {"t": (1, "two", None)}
    assert _wire(flaky_server, payload) == _stdlib(payload)


def test_int_past_i64_but_within_u64(flaky_server):
    payload = {"n": 2**63}
    assert _wire(flaky_server, payload) == _stdlib(payload)


def test_i64_min_encodes_exactly(flaky_server):
    payload = {"n": -(2**63)}
    assert _wire(flaky_server, payload) == _stdlib(payload)


def test_int_dict_key_is_coerced_to_str(flaky_server):
    payload = {1: "a"}
    assert _wire(flaky_server, payload) == _stdlib(payload)


def test_bool_none_and_float_keys_are_coerced_like_stdlib(flaky_server):
    payload = {True: 1, False: 2, None: 3, 1.5: 4}
    assert _wire(flaky_server, payload) == '{"true":1,"false":2,"null":3,"1.5":4}'


def test_int_enum_encodes_as_its_int(flaky_server):
    class Color(IntEnum):
        RED = 1

    assert _wire(flaky_server, {"c": Color.RED}) == '{"c":1}'


def test_str_subclass_encodes_as_str(flaky_server):
    class Tag(str):
        pass

    assert _wire(flaky_server, {"t": Tag("x")}) == '{"t":"x"}'


def test_unicode_is_not_escaped(flaky_server):
    payload = {"s": "héllo ✓"}
    assert _wire(flaky_server, payload) == _stdlib(payload)


def test_nested_structure_round_trips(flaky_server):
    payload = {"a": [1, {"b": [True, None, "x"]}, []], "c": {}, "d": ""}
    assert _wire(flaky_server, payload) == _stdlib(payload)


def test_empty_containers(flaky_server):
    assert _wire(flaky_server, {}) == "{}"
    assert _wire(flaky_server, []) == "[]"


def test_top_level_scalars(flaky_server):
    """`json=None` means no body at the API, so the literal `null` is only
    reachable nested; top-level str and int still encode."""
    assert _wire(flaky_server, "s") == '"s"'
    assert _wire(flaky_server, 1) == "1"
    assert _wire(flaky_server, [None]) == "[null]"


def test_response_json_keeps_document_order(flaky_server):
    """serde_json's `preserve_order` also applies on decode: `.json()` returns
    keys in document order like `json.loads`, not sorted."""
    resp = rqx.post(f"{flaky_server}/echo-body", json={"x": 1})
    assert list(resp.json().keys()) == ["method", "content_type", "body"]


def test_floats_are_value_equivalent(flaky_server):
    """serde_json and Python print some floats differently (`1e16` vs
    `1e+16`), so floats are compared after parsing, not byte-for-byte."""
    payload = {"a": 1.0, "b": 0.1, "c": 1e16, "d": -2.5e-7}
    assert json.loads(_wire(flaky_server, payload)) == payload


# --- errors match stdlib's exception types ------------------------------------


@pytest.mark.parametrize("n", [2**64, -(2**63) - 1])
def test_int_past_64_bits_raises_overflow_error(flaky_server, n):
    with pytest.raises(OverflowError):
        rqx.post(f"{flaky_server}/echo-body", json={"n": n})


@pytest.mark.parametrize("f", [float("nan"), float("inf"), float("-inf")])
def test_non_finite_float_raises_value_error(flaky_server, f):
    with pytest.raises(
        ValueError, match="Out of range float values are not JSON compliant"
    ):
        rqx.post(f"{flaky_server}/echo-body", json={"x": f})


@pytest.mark.parametrize(
    "value,type_name",
    [
        (b"x", "bytes"),
        ({1, 2}, "set"),
        (object(), "object"),
        (datetime(2026, 1, 1, tzinfo=timezone.utc), "datetime"),
    ],
)
def test_unsupported_value_raises_type_error(flaky_server, value, type_name):
    with pytest.raises(
        TypeError, match=f"Object of type {type_name} is not JSON serializable"
    ):
        rqx.post(f"{flaky_server}/echo-body", json={"v": value})


@pytest.mark.parametrize("f", [float("nan"), float("inf"), float("-inf")])
def test_non_finite_float_dict_key_raises_value_error(flaky_server, f):
    """stdlib with allow_nan=False rejects a NaN key the same way as a value."""
    with pytest.raises(
        ValueError, match="Out of range float values are not JSON compliant"
    ):
        rqx.post(f"{flaky_server}/echo-body", json={f: 1})


def test_keys_that_collide_after_coercion_keep_the_last_value(flaky_server):
    """stdlib writes both keys (`{"1":"a","1":"b"}`); a receiver parsing that
    gets last-wins, which is what serde's map yields directly. Pinned so a
    change here is deliberate."""
    assert _wire(flaky_server, {1: "a", "1": "b"}) == '{"1":"b"}'


def test_unsupported_dict_key_raises_type_error(flaky_server):
    with pytest.raises(
        TypeError, match="keys must be str, int, float, bool or None, not tuple"
    ):
        rqx.post(f"{flaky_server}/echo-body", json={(1, 2): "a"})


def test_deep_nesting_raises_recursion_error(flaky_server):
    """stdlib raises RecursionError around 1000 levels; a stack overflow
    in the encoder would take the whole process down instead."""
    payload = []
    for _ in range(100_000):
        payload = [payload]
    with pytest.raises(RecursionError):
        rqx.post(f"{flaky_server}/echo-body", json=payload)


def test_circular_reference_raises_value_error(flaky_server):
    payload = {}
    payload["self"] = payload
    with pytest.raises(ValueError, match="Circular reference detected"):
        rqx.post(f"{flaky_server}/echo-body", json=payload)


@pytest.mark.asyncio
async def test_async_client_shares_the_encoder(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.post(f"{flaky_server}/echo-body", json={"b": 1, "a": (2,)})
        assert resp.json()["body"] == '{"b":1,"a":[2]}'
        with pytest.raises(OverflowError):
            await client.post(f"{flaky_server}/echo-body", json={"n": 2**64})


# --- decode side: integers at the 64-bit boundaries -------------------------


def test_response_json_decodes_u64_range_ints_exactly(flaky_server):
    """Literals between 2^63 and 2^64 - 1 fit serde_json's u64 and must come
    back as exact ints, not floats."""
    body = rqx.get(f"{flaky_server}/big-ints").json()
    assert body["i64_max"] == 2**63 - 1
    assert body["u64_min"] == 2**63
    assert body["u64_max"] == 2**64 - 1
    assert all(isinstance(body[k], int) for k in ("i64_max", "u64_min", "u64_max"))


def test_response_json_past_u64_is_a_float(flaky_server):
    """Documented divergence: past 2^64 serde_json parses the literal as f64,
    so the value is rounded where stdlib returns an exact int. The fixture
    serves 2^64 + 1, which is not representable as f64 and rounds to 2^64.
    Tracked for the migration guide, https://github.com/rodcochran/rqx/issues/116."""
    body = rqx.get(f"{flaky_server}/big-ints").json()
    assert isinstance(body["past_u64"], float)
    assert body["past_u64"] == float(2**64)
    assert body["past_u64"] != 2**64 + 1
