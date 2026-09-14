"""Streaming memory stays bounded when the consumer is slower than the network:
hyper reads the socket only when the body is polled, and `chunk_size` bounds
what the iterator holds (https://github.com/rodcochran/rqx/issues/107)."""

import asyncio
import resource
import time

import pytest

import rqx

BODY_MIB = 128
CHUNK = 1 << 20
SLOW_CHUNKS = 20  # ~1 s of sleeping while the server has the whole body ready
# 1 MiB pieces cost ~30 MiB of peak RSS on macOS (carry buffer, one network
# chunk, the Python bytes in flight, allocator slack). Unbounded reading would
# show as the whole 128 MiB body, so the line sits well clear of both.
ALLOWED_GROWTH_MIB = 64


def peak_rss_mib() -> float:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1 << 20)


@pytest.mark.slow
def test_slow_sync_consumer_does_not_grow_memory(flaky_server):
    before = peak_rss_mib()
    total = 0
    with rqx.Client().stream("GET", f"{flaky_server}/large/{BODY_MIB}") as resp:
        for i, chunk in enumerate(resp.iter_bytes(CHUNK)):
            total += len(chunk)
            if i < SLOW_CHUNKS:
                time.sleep(0.05)
    assert total == BODY_MIB << 20
    assert peak_rss_mib() - before < ALLOWED_GROWTH_MIB


@pytest.mark.slow
@pytest.mark.asyncio
async def test_slow_async_consumer_does_not_grow_memory(flaky_server):
    before = peak_rss_mib()
    total = 0
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", f"{flaky_server}/large/{BODY_MIB}") as resp:
            i = 0
            async for chunk in resp.aiter_bytes(CHUNK):
                total += len(chunk)
                if i < SLOW_CHUNKS:
                    await asyncio.sleep(0.05)
                i += 1
    assert total == BODY_MIB << 20
    assert peak_rss_mib() - before < ALLOWED_GROWTH_MIB
