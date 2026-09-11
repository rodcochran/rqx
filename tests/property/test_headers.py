"""`headers=` and the Headers class: valid input reaches the wire intact,
invalid names always raise, lookups ignore case."""

from hypothesis import given

import rqx
from tests.property.strategies import (
    header_name_with_one_bad_char,
    header_value,
    headers,
)


def _sent(resp):
    out = {}
    for name, value in resp.json():
        out.setdefault(name.lower(), []).append(value)
    return out


@given(headers)
def test_valid_headers_reach_the_server_grouped_by_name(flaky_server, client, mapping):
    resp = client.get(f"{flaky_server}/echo-headers", headers=mapping)
    sent = _sent(resp)
    expected = {}
    for name, value in mapping.items():
        expected.setdefault(name.lower(), []).append(value)
    for name, values in expected.items():
        assert sent[name] == values


@given(header_name_with_one_bad_char(), header_value)
def test_any_non_token_char_in_a_name_raises_value_error(
    flaky_server, client, name, value
):
    try:
        client.get(f"{flaky_server}/echo-headers", headers={name: value})
    except ValueError as e:
        assert "invalid header name" in str(e)
    else:
        raise AssertionError(f"accepted {name!r}")


@given(headers)
def test_headers_class_lookup_ignores_case(mapping):
    h = rqx.Headers(mapping)
    distinct = {k.lower() for k in mapping}
    assert len(h) == len(distinct)
    for k in mapping:
        assert k.upper() in h
        assert k.lower() in h
        assert h[k.upper()] == h[k.lower()]
