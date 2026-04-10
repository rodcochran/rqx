import asyncio
import time

import pytest
import reqx
from rich import print

HTTPBIN_HOST = "http://localhost"


@pytest.mark.asyncio
async def test_context_mangers():
    async with reqx.AsyncClient() as client:
        assert client is not None


@pytest.mark.asyncio
async def test_get():
    async with reqx.AsyncClient() as client:
        assert client is not None

        future = client.get(f"{HTTPBIN_HOST}/get")
        resp = await future

        assert resp.status_code == 200
        assert "content-type" in resp.headers
        body = resp.json()
        assert body["url"] == f"{HTTPBIN_HOST}/get"
        print("")
        print(f"JSON body:\n{body}")


@pytest.mark.asyncio
async def test_concurrent_gets():
    async with reqx.AsyncClient() as client:
        assert client is not None

        async def task(wait_time):
            fut = client.get(f"{HTTPBIN_HOST}/delay/{wait_time}")
            return await fut

        durations = [1, 2, 3, 4, 5]
        futures = [task(d) for d in durations]
        start = time.perf_counter()
        resp_list = await asyncio.gather(*futures)
        end = time.perf_counter()
        duration = end - start

        assert duration < (max(durations) * 1.1)
        print(f"Concurrent tasks duration: {duration}s")

        for resp in resp_list:
            assert resp.status_code == 200
            assert "content-type" in resp.headers
            assert resp.json() is not None
