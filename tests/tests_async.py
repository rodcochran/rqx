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


@pytest.mark.asyncio
async def test_post():
    async with reqx.AsyncClient() as client:
        assert client is not None

        future = client.post(f"{HTTPBIN_HOST}/post")
        resp = await future

        assert resp.status_code == 200
        assert "content-type" in resp.headers
        body = resp.json()
        assert body["url"] == f"{HTTPBIN_HOST}/post"
        print("")
        print(f"JSON body:\n{body}")


@pytest.mark.asyncio
async def test_patch():
    async with reqx.AsyncClient() as client:
        assert client is not None

        future = client.patch(f"{HTTPBIN_HOST}/patch")
        resp = await future

        assert resp.status_code == 200
        assert "content-type" in resp.headers
        body = resp.json()
        assert body["url"] == f"{HTTPBIN_HOST}/patch"
        print("")
        print(f"JSON body:\n{body}")


@pytest.mark.asyncio
async def test_put():
    async with reqx.AsyncClient() as client:
        assert client is not None

        future = client.put(f"{HTTPBIN_HOST}/put")
        resp = await future

        assert resp.status_code == 200
        assert "content-type" in resp.headers
        body = resp.json()
        assert body["url"] == f"{HTTPBIN_HOST}/put"
        print("")
        print(f"JSON body:\n{body}")


@pytest.mark.asyncio
async def test_delete():
    async with reqx.AsyncClient() as client:
        assert client is not None

        future = client.delete(f"{HTTPBIN_HOST}/delete")
        resp = await future

        assert resp.status_code == 200
        assert "content-type" in resp.headers
        body = resp.json()
        assert body["url"] == f"{HTTPBIN_HOST}/delete"
        print("")
        print(f"JSON body:\n{body}")


@pytest.mark.asyncio
async def test_options():
    async with reqx.AsyncClient() as client:
        assert client is not None

        future = client.options(f"{HTTPBIN_HOST}/get")
        resp = await future

        assert resp.status_code == 200
        assert "content-type" in resp.headers
        assert "allow" in resp.headers


@pytest.mark.asyncio
async def test_sample_json_params_post():
    client = reqx.AsyncClient()
    resp = await client.post(f"{HTTPBIN_HOST}/post", json={"special_param": 1})
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Post JSON response:\n{body}")


@pytest.mark.asyncio
async def test_basic_client_based_redirect():
    client = reqx.AsyncClient(follow_redirects=True)
    resp = await client.get(
        f"{HTTPBIN_HOST}/redirect/3",
    )
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_basic_request_based_redirect():
    client = reqx.AsyncClient(follow_redirects=False)
    resp = await client.get(
        f"{HTTPBIN_HOST}/redirect/3",
        follow_redirects=True,
    )
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_false_follow_redirects_returns_302():
    client = reqx.AsyncClient(follow_redirects=False)
    resp = await client.get(
        f"{HTTPBIN_HOST}/redirect/3",
        follow_redirects=False,
    )
    assert resp.status_code == 302


@pytest.mark.asyncio
async def test_raise_error_on_redirects_exeeding_max_redirects():
    client = reqx.AsyncClient(follow_redirects=True, max_redirects=1)
    with pytest.raises(reqx.TooManyRedirects):
        await client.get(f"{HTTPBIN_HOST}/redirect/3")


@pytest.mark.asyncio
async def test_raise_for_status():
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/status/400")
    assert resp.status_code == 400
    assert "content-type" in resp.headers
    with pytest.raises(reqx.HTTPStatusError):
        resp.raise_for_status()


@pytest.mark.asyncio
async def test_post_with_content():
    content_str = '{"raw_content": "hello"}'
    content_bytes = content_str.encode()
    client = reqx.AsyncClient()
    resp = await client.post(f"{HTTPBIN_HOST}/post", content=content_bytes)
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    assert body["data"] == content_str
    print("")
    print(f"Post JSON response:\n{body}")


@pytest.mark.asyncio
async def test_post_with_data():
    data = {"hi": "goodbye", "hey": "2"}
    client = reqx.AsyncClient()
    resp = await client.post(f"{HTTPBIN_HOST}/post", data=data)
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()
    assert body["headers"]["Content-Type"] == "application/x-www-form-urlencoded"
    print("")
    print(f"Post JSON response:\n{body}")


@pytest.mark.asyncio
async def test_basic_auth():
    u = "User"
    p = "Password"
    auth = (u, p)
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/basic-auth/{u}/{p}", auth=auth)
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_400():
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/status/400")
    assert resp.status_code == 400
    assert "content-type" in resp.headers
    with pytest.raises(reqx.ReqxError):
        resp.json()


@pytest.mark.asyncio
async def test_404():
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/status/404")
    assert resp.status_code == 404
    assert "content-type" in resp.headers
    with pytest.raises(reqx.ReqxError):
        resp.json()


@pytest.mark.asyncio
async def test_500():
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/status/500")
    assert resp.status_code == 500
    assert "content-type" in resp.headers
    with pytest.raises(reqx.ReqxError):
        resp.json()


@pytest.mark.asyncio
async def test_body():
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/get")
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()

    expected_body_keys = ["args", "headers", "origin", "url"]

    for k in expected_body_keys:
        assert k in body.keys()


@pytest.mark.asyncio
async def test_basic_final_url_in_output():
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/get")
    assert resp.url == f"{HTTPBIN_HOST}/get"


@pytest.mark.asyncio
async def test_redirected_final_url_in_output():
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/redirect/3", follow_redirects=True)
    assert resp.url == f"{HTTPBIN_HOST}/get"


@pytest.mark.asyncio
async def test_bad_url_raises():
    client = reqx.AsyncClient()
    with pytest.raises(reqx.ReqxError):
        await client.get("Bad URL")


@pytest.mark.asyncio
async def test_get_total_elapsed_time():
    delay_time = 1
    client = reqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/delay/{delay_time}")
    assert resp.elapsed is not None
    assert resp.elapsed > delay_time
    print("")
    print(f"Elapsed time:\n{resp.elapsed:.2f}s")
