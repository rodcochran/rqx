"""`params=` for any str/int/float/bool/None mapping round-trips through the
query string the way httpx would send it."""

from urllib.parse import parse_qsl

from hypothesis import given
from hypothesis import strategies as st

import rqx
from tests.property.strategies import params


def _httpx_pairs(mapping):
    """httpx's primitive_value_to_str, applied like QueryParams does."""
    out = []
    for k, v in mapping.items():
        if v is None:
            continue
        if v is True:
            out.append((k, "true"))
        elif v is False:
            out.append((k, "false"))
        else:
            out.append((k, str(v)))
    return out


@given(params)
def test_params_round_trip_as_httpx_would_send_them(flaky_server, mapping):
    resp = rqx.get(f"{flaky_server}/echo-url/p", params=mapping)
    query = resp.json()["query"]
    assert parse_qsl(query, keep_blank_values=True) == _httpx_pairs(mapping)


@given(
    st.one_of(
        st.binary(),
        st.lists(st.integers()),
        st.tuples(st.integers()),
        st.dictionaries(st.text(), st.text()),
    )
)
def test_unsupported_param_value_types_raise_type_error(flaky_server, value):
    try:
        rqx.get(f"{flaky_server}/echo-url/p", params={"k": value})
    except TypeError as e:
        assert "str, int, float, bool, or None" in str(e)
    else:
        raise AssertionError(f"accepted {type(value).__name__}")
