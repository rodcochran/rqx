"""`json=` encodes like stdlib and `.json()` decodes back to the same value,
key order included."""

import json

from hypothesis import assume, given
from hypothesis import strategies as st

import rqx
from tests.property.strategies import json_value, json_value_no_floats


def _wire(flaky_server, payload):
    return rqx.post(f"{flaky_server}/echo-body", json=payload).json()["body"]


@given(json_value_no_floats)
def test_encode_matches_stdlib_byte_for_byte(flaky_server, payload):
    assume(payload is not None)  # json=None means no body, not the literal null
    expected = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    assert _wire(flaky_server, payload) == expected


@given(json_value)
def test_encode_is_value_equivalent_to_stdlib(flaky_server, payload):
    """Floats may print differently (`1e16` vs `1e+16`) but must parse equal."""
    assume(payload is not None)
    assert json.loads(_wire(flaky_server, payload)) == payload


@given(json_value)
def test_decode_round_trips_with_key_order(flaky_server, payload):
    """Needs serde_json's `float_roundtrip`: its default float parser is
    best-effort and hypothesis found a literal that came back one ulp off."""
    body = json.dumps(payload).encode()
    resp = rqx.post(f"{flaky_server}/reflect-json", content=body)
    assert json.dumps(resp.json()) == json.dumps(payload)


@given(st.one_of(st.integers(max_value=-(2**63) - 1), st.integers(min_value=2**64)))
def test_ints_past_64_bits_always_raise_overflow_error(flaky_server, n):
    try:
        rqx.post(f"{flaky_server}/echo-body", json={"n": n})
    except OverflowError:
        pass
    else:
        raise AssertionError(f"encoded {n}")
