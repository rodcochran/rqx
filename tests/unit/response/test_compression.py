import pytest

import rqx


@pytest.mark.parametrize("algorithm", ["gzip", "deflate", "brotli", "zstd"])
def test_compressed_response_is_decompressed(flaky_server, algorithm):
    """Server returns a compressed response; rqx decompresses transparently.

    The server-side handler compresses a known payload with the requested
    algorithm and sets the appropriate Content-Encoding header. If reqwest's
    compression features are wired up correctly, .json() returns the original
    payload — if not, the JSON parser fails on compressed bytes.
    """
    resp = rqx.Client().get(f"{flaky_server}/compressed/{algorithm}")
    assert resp.status_code == 200
    assert resp.json() == {"compressed": True, "algorithm": algorithm}


@pytest.mark.asyncio
@pytest.mark.parametrize("algorithm", ["gzip", "deflate", "brotli", "zstd"])
async def test_compressed_response_is_decompressed_async(flaky_server, algorithm):
    client = rqx.AsyncClient()
    resp = await client.get(f"{flaky_server}/compressed/{algorithm}")
    assert resp.status_code == 200
    assert resp.json() == {"compressed": True, "algorithm": algorithm}
