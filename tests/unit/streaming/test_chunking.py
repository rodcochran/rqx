"""`chunk_size` is honored when given: byte chunks are exactly that size except
the last, text chunks count characters, lines don't take one. Without it, chunks
pass through as the network delivered them (https://github.com/rodcochran/rqx/issues/107)."""

import pytest

import rqx

BODY_LEN = 100_003  # not a multiple of any chunk size below


def pattern(n: int) -> bytes:
    return bytes(range(256)) * (n // 256) + bytes(range(n % 256))


def check_chunks(chunks, chunk_size, expected):
    assert chunks, "expected at least one chunk"
    assert all(len(c) == chunk_size for c in chunks[:-1]), (
        "every chunk but the last is exact"
    )
    assert 0 < len(chunks[-1]) <= chunk_size
    assert b"".join(chunks) == expected


@pytest.mark.parametrize("chunk_size", [1, 7, 8192, 65_536, BODY_LEN, BODY_LEN + 1])
def test_iter_bytes_yields_exact_chunks(flaky_server, chunk_size):
    with rqx.Client().stream("GET", f"{flaky_server}/bytes/{BODY_LEN}") as resp:
        chunks = list(resp.iter_bytes(chunk_size))
    check_chunks(chunks, chunk_size, pattern(BODY_LEN))


@pytest.mark.parametrize(
    "chunk_size", [7, 8192, 65_536, BODY_LEN, BODY_LEN + 1]
)  # 1: sync twin
@pytest.mark.asyncio
async def test_aiter_bytes_yields_exact_chunks(flaky_server, chunk_size):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/bytes/{BODY_LEN}") as resp:
            chunks = [c async for c in resp.aiter_bytes(chunk_size)]
    check_chunks(chunks, chunk_size, pattern(BODY_LEN))


def test_default_reassembles_without_a_chunk_size(flaky_server):
    """No chunk_size means no regrouping, like httpx. Piece boundaries then belong
    to the transport, so only reassembly is asserted here; the pass-through itself
    is pinned by the Rust unit test `chunkers_without_a_size_pass_chunks_through`."""
    with rqx.Client().stream("GET", f"{flaky_server}/bytes/{BODY_LEN}") as resp:
        chunks = list(resp.iter_bytes())
    assert chunks
    assert b"".join(chunks) == pattern(BODY_LEN)


def test_empty_body_yields_nothing(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/bytes/0") as resp:
        assert list(resp.iter_bytes(16)) == []


@pytest.mark.parametrize("chunk_size", [1, 3, 1000])
def test_iter_text_counts_characters_and_never_splits_one(flaky_server, chunk_size):
    with rqx.Client().stream("GET", f"{flaky_server}/bigtext") as resp:
        chunks = list(resp.iter_text(chunk_size))
    assert all(len(c) == chunk_size for c in chunks[:-1])
    assert 0 < len(chunks[-1]) <= chunk_size
    text = "".join(chunks)
    assert text == "aé€🙂" * 100_000
    assert "�" not in text


@pytest.mark.parametrize(
    "chunk_size", [50, 1000]
)  # tiny sizes mean 100k+ awaits; the sync twin covers them
@pytest.mark.asyncio
async def test_aiter_text_counts_characters_and_never_splits_one(
    flaky_server, chunk_size
):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/bigtext") as resp:
            chunks = [c async for c in resp.aiter_text(chunk_size)]
    assert all(len(c) == chunk_size for c in chunks[:-1])
    assert "".join(chunks) == "aé€🙂" * 100_000


def test_iter_lines_takes_no_chunk_size(flaky_server):
    """httpx's iter_lines has no chunk_size; ours used to accept and ignore one."""
    with rqx.Client().stream("GET", f"{flaky_server}/lines") as resp:
        assert list(resp.iter_lines()) == ["first", "second", "third"]
        with pytest.raises(TypeError):
            resp.iter_lines(8192)


@pytest.mark.asyncio
async def test_aiter_lines_takes_no_chunk_size(flaky_server):
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/lines") as resp:
            with pytest.raises(TypeError):
                resp.aiter_lines(8192)
            assert [line async for line in resp.aiter_lines()] == [
                "first",
                "second",
                "third",
            ]


def test_close_discards_buffered_bytes(flaky_server):
    """A close with bytes still in the rechunk buffer raises rather than yielding stale data."""
    with rqx.Client().stream("GET", f"{flaky_server}/bytes/{BODY_LEN}") as resp:
        chunks = resp.iter_bytes(10)
        assert (
            len(next(chunks)) == 10
        )  # the network chunk was bigger; a remainder is buffered
        resp.close()
        with pytest.raises(rqx.RqxError, match="closed"):
            next(chunks)


def test_exhausted_iterator_stays_exhausted(flaky_server):
    with rqx.Client().stream("GET", f"{flaky_server}/bytes/100") as resp:
        chunks = resp.iter_bytes(64)
        assert [len(c) for c in chunks] == [64, 36]
        assert next(chunks, None) is None
        assert next(chunks, None) is None
