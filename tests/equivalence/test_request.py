"""Request-building edges where the two libraries knowingly disagree."""

import pytest

ISSUE_116 = "https://github.com/rodcochran/rqx/issues/116"


@pytest.mark.rqx_diverges(
    issue=ISSUE_116,
    reason="rqx sends non-ASCII header values as UTF-8 obs-text; httpx rejects them",
)
def test_non_ascii_header_value_is_rejected(lib, flaky_server):
    with pytest.raises(Exception):
        lib.client().get(f"{flaky_server}/echo-headers", headers={"X-T": "héllo"})


@pytest.mark.rqx_diverges(
    issue=ISSUE_116, reason="rqx header values are str-only; httpx accepts bytes"
)
def test_bytes_header_value_is_accepted(lib, flaky_server):
    resp = lib.client().get(f"{flaky_server}/echo-headers", headers={"X-T": b"v"})
    assert dict((k.lower(), v) for k, v in resp.json())["x-t"] == "v"


@pytest.mark.rqx_diverges(
    issue=ISSUE_116,
    reason="rqx raises OverflowError past 64-bit ints; stdlib json serializes them",
)
def test_json_int_past_64_bits_is_serialized(lib, flaky_server):
    resp = lib.client().post(f"{flaky_server}/echo-body", json={"n": 2**64})
    assert resp.json()["body"] == '{"n":18446744073709551616}'
