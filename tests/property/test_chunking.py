"""Any chunk_size regroups any body into exact-size pieces that reassemble to the body."""

from hypothesis import given, settings
from hypothesis import strategies as st

BODY_LEN = st.integers(min_value=0, max_value=200_000)
CHUNK_SIZE = st.integers(min_value=1, max_value=70_000)


def pattern(n: int) -> bytes:
    return bytes(range(256)) * (n // 256) + bytes(range(n % 256))


@settings(deadline=None)
@given(BODY_LEN, CHUNK_SIZE)
def test_iter_bytes_regroups_any_body(flaky_server, client, body_len, chunk_size):
    with client.stream("GET", f"{flaky_server}/bytes/{body_len}") as resp:
        chunks = list(resp.iter_bytes(chunk_size))
    assert all(len(c) == chunk_size for c in chunks[:-1])
    assert all(chunks) and (not chunks or len(chunks[-1]) <= chunk_size)
    assert b"".join(chunks) == pattern(body_len)
