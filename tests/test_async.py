import asyncio
import time

import pytest
import rqx
from rich import print

HTTPBIN_HOST = "http://localhost"


@pytest.mark.asyncio
async def test_context_mangers():
    async with rqx.AsyncClient() as client:
        assert client is not None


@pytest.mark.asyncio
async def test_get():
    async with rqx.AsyncClient() as client:
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
    async with rqx.AsyncClient() as client:
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
    async with rqx.AsyncClient() as client:
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
    async with rqx.AsyncClient() as client:
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
    async with rqx.AsyncClient() as client:
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
    async with rqx.AsyncClient() as client:
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
    async with rqx.AsyncClient() as client:
        assert client is not None

        future = client.options(f"{HTTPBIN_HOST}/get")
        resp = await future

        assert resp.status_code == 200
        assert "content-type" in resp.headers
        assert "allow" in resp.headers


@pytest.mark.asyncio
async def test_sample_json_params_post():
    client = rqx.AsyncClient()
    resp = await client.post(f"{HTTPBIN_HOST}/post", json={"special_param": 1})
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Post JSON response:\n{body}")


@pytest.mark.asyncio
async def test_basic_client_based_redirect():
    client = rqx.AsyncClient(follow_redirects=True)
    resp = await client.get(
        f"{HTTPBIN_HOST}/redirect/3",
    )
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_basic_request_based_redirect():
    client = rqx.AsyncClient(follow_redirects=False)
    resp = await client.get(
        f"{HTTPBIN_HOST}/redirect/3",
        follow_redirects=True,
    )
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_false_follow_redirects_returns_302():
    client = rqx.AsyncClient(follow_redirects=False)
    resp = await client.get(
        f"{HTTPBIN_HOST}/redirect/3",
        follow_redirects=False,
    )
    assert resp.status_code == 302


@pytest.mark.asyncio
async def test_raise_error_on_redirects_exeeding_max_redirects():
    client = rqx.AsyncClient(follow_redirects=True, max_redirects=1)
    with pytest.raises(rqx.TooManyRedirects):
        await client.get(f"{HTTPBIN_HOST}/redirect/3")


@pytest.mark.asyncio
async def test_raise_for_status():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/status/400")
    assert resp.status_code == 400
    assert "content-type" in resp.headers
    with pytest.raises(rqx.HTTPStatusError):
        resp.raise_for_status()


@pytest.mark.asyncio
async def test_post_with_content():
    content_str = '{"raw_content": "hello"}'
    content_bytes = content_str.encode()
    client = rqx.AsyncClient()
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
    client = rqx.AsyncClient()
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
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/basic-auth/{u}/{p}", auth=auth)
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_400():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/status/400")
    assert resp.status_code == 400
    assert "content-type" in resp.headers
    with pytest.raises(rqx.RqxError):
        resp.json()


@pytest.mark.asyncio
async def test_404():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/status/404")
    assert resp.status_code == 404
    assert "content-type" in resp.headers
    with pytest.raises(rqx.RqxError):
        resp.json()


@pytest.mark.asyncio
async def test_500():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/status/500")
    assert resp.status_code == 500
    assert "content-type" in resp.headers
    with pytest.raises(rqx.RqxError):
        resp.json()


@pytest.mark.asyncio
async def test_body():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/get")
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()

    expected_body_keys = ["args", "headers", "origin", "url"]

    for k in expected_body_keys:
        assert k in body.keys()


@pytest.mark.asyncio
async def test_basic_final_url_in_output():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/get")
    assert resp.url == f"{HTTPBIN_HOST}/get"


@pytest.mark.asyncio
async def test_redirected_final_url_in_output():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/redirect/3", follow_redirects=True)
    assert resp.url == f"{HTTPBIN_HOST}/get"


@pytest.mark.asyncio
async def test_bad_url_raises():
    client = rqx.AsyncClient()
    with pytest.raises(rqx.RqxError):
        await client.get("Bad URL")


@pytest.mark.asyncio
async def test_get_total_elapsed_time():
    delay_time = 1
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/delay/{delay_time}")
    assert resp.elapsed is not None
    assert resp.elapsed > delay_time
    print("")
    print(f"Elapsed time:\n{resp.elapsed:.2f}s")


@pytest.mark.asyncio
async def test_valid_text():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/get")
    text = resp.text
    assert text is not None
    assert isinstance(text, str)
    assert len(text) > 0
    print("")
    print(f"Text:\n{text}")


@pytest.mark.asyncio
async def test_valid_bytes():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/get")
    content = resp.content
    assert content is not None
    assert isinstance(content, bytes)
    assert not isinstance(content, list)
    print("")
    print(f"Content:\n{content}")


@pytest.mark.asyncio
async def test_headers():
    client = rqx.AsyncClient()
    resp = await client.get(f"{HTTPBIN_HOST}/get")
    headers = resp.headers
    assert headers is not None
    assert isinstance(headers, rqx.Headers)
    assert headers["content-type"] == "application/json"
    print("")
    print(f"Headers:\n{headers}")


@pytest.mark.asyncio
async def test_nested_json():
    # httpbin's /get?foo=bar&baz=123 will give you query params in the args field.
    # Good way to test that nested JSON values come through correctly.
    client = rqx.AsyncClient()

    key1 = "baz"
    val1 = "123"
    key2 = "foo"
    val2 = "bar"

    resp = await client.get(f"{HTTPBIN_HOST}/get?{key1}={val1}&{key2}={val2}")
    body = resp.json()
    args = body["args"]
    assert args[key1] == val1
    assert args[key2] == val2
    print("")
    print(f"Nested Json (body args):\n{body}")


@pytest.mark.asyncio
async def test_get_with_timeout():
    client = rqx.AsyncClient()
    with pytest.raises(rqx.TimeoutException):
        await client.get(f"{HTTPBIN_HOST}/delay/5", timeout=1)


# ================================================================
# Phase 3 tests
# ================================================================


@pytest.mark.asyncio
async def test_transport_init():

    transport = rqx.AsyncHTTPTransport()

    assert transport is not None
    assert transport.retries is None


@pytest.mark.asyncio
async def test_retry_on_flaky_server(flaky_server):
    transport = rqx.AsyncHTTPTransport(
        retries=rqx.Retry(
            total=5,
            backoff_factor=0.1,
            status_forcelist={503},
        )
    )
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(f"{flaky_server}/flaky?request_id=asynctest1")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_exceeded_retries_on_flaky_server(flaky_server):
    transport = rqx.AsyncHTTPTransport(
        retries=rqx.Retry(
            total=1,
            backoff_factor=0.1,
            status_forcelist={503},
        )
    )
    client = rqx.AsyncClient(transport=transport)

    with pytest.raises(rqx.MaxRetriesExceeded):
        await client.get(f"{flaky_server}/flaky?request_id=asynctest2")


@pytest.mark.asyncio
async def test_404_is_not_retried():
    transport = rqx.AsyncHTTPTransport(
        retries=rqx.Retry(
            total=1,
            backoff_factor=0.1,
            status_forcelist={503},
        )
    )
    client = rqx.AsyncClient(transport=transport)

    resp = await client.get(f"{HTTPBIN_HOST}/status/404")
    assert resp.status_code == 404
    assert "content-type" in resp.headers
    with pytest.raises(rqx.RqxError):
        resp.json()


@pytest.mark.asyncio
async def test_not_allowed_method_is_not_retried(flaky_server):
    transport = rqx.AsyncHTTPTransport(
        retries=rqx.Retry(
            total=1,
            backoff_factor=0.1,
            status_forcelist={503},
            allowed_methods={"POST"},
        )
    )
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(f"{flaky_server}/flaky?request_id=asynctest3")

    assert resp.status_code == 503
    assert "content-type" in resp.headers


@pytest.mark.asyncio
async def test_retry_history_populated(flaky_server):
    transport = rqx.AsyncHTTPTransport(
        retries=rqx.Retry(
            total=5,
            backoff_factor=0.1,
            status_forcelist={503},
        )
    )
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(f"{flaky_server}/flaky?request_id=asynctest4")
    assert resp.status_code == 200
    assert resp.num_retries == 2
    assert len(resp.retry_history) == 2
    assert resp.retry_history[0][0] == "503"  # status code string
    print("")
    print(f"Retry History:\n{resp.retry_history}")


@pytest.mark.asyncio
async def test_total_timeout_exceeded(flaky_server):
    transport = rqx.AsyncHTTPTransport(
        retries=rqx.Retry(
            total=5,
            backoff_factor=2.0,
            status_forcelist={503},
            total_timeout=1.0,
        )
    )
    client = rqx.AsyncClient(transport=transport)
    with pytest.raises(rqx.MaxRetriesExceeded):
        await client.get(f"{flaky_server}/flaky?request_id=asynctest5")


@pytest.mark.asyncio
async def test_total_timeout_not_exceeded(flaky_server):
    transport = rqx.AsyncHTTPTransport(
        retries=rqx.Retry(
            total=5,
            backoff_factor=0.1,
            status_forcelist={503},
            total_timeout=30.0,
        )
    )
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(f"{flaky_server}/flaky?request_id=asynctest6")
    assert resp.status_code == 200
    assert resp.num_retries == 2
    assert len(resp.retry_history) == 2
    assert resp.retry_history[0][0] == "503"  # status code string
    print("")
    print(f"Retry History:\n{resp.retry_history}")


# ================================================================
# Phase 4 tests
# ================================================================


@pytest.mark.asyncio
async def test_max_connections():
    async with rqx.AsyncClient(
        transport=rqx.AsyncHTTPTransport(max_connections=2)
    ) as client:
        assert client is not None

        async def task(wait_time):
            fut = client.get(f"{HTTPBIN_HOST}/delay/{wait_time}")
            return await fut

        durations = [1, 1, 1, 1, 1]
        futures = [task(d) for d in durations]
        start = time.perf_counter()
        resp_list = await asyncio.gather(*futures)
        end = time.perf_counter()
        duration = end - start

        print(f"Concurrent tasks duration: {duration}s")

        all_parallel_time = max(durations)
        all_serial_time = sum(durations)
        print(
            f"All Parallel Time: {all_parallel_time}s\n"
            f"Duration: {duration}s\n"
            f"All Serial: {all_serial_time}s"
        )

        assert all_parallel_time < duration < all_serial_time

        for resp in resp_list:
            assert resp.status_code == 200
            assert "content-type" in resp.headers
            assert resp.json() is not None


@pytest.mark.asyncio
async def test_basic_http2():
    transport = rqx.AsyncHTTPTransport(http2=True)
    client = rqx.AsyncClient(transport=transport)
    url = "https://nghttp2.org/httpbin/get"
    resp = await client.get(url=url)
    assert resp.http_version == "HTTP/2.0"


@pytest.mark.asyncio
async def test_basic_http2_explicit_opt_out():
    transport = rqx.AsyncHTTPTransport(http2=False)
    client = rqx.AsyncClient(transport=transport)
    url = "https://nghttp2.org/httpbin/get"
    resp = await client.get(url=url)
    assert resp.http_version != "HTTP/2.0"


@pytest.mark.asyncio
async def test_basic_http2_default_negotiates_h2():
    # No http1/http2 kwargs → ALPN. Against h2-capable server → h2.
    transport = rqx.AsyncHTTPTransport()
    client = rqx.AsyncClient(transport=transport)
    url = "https://nghttp2.org/httpbin/get"
    resp = await client.get(url=url)
    assert resp.http_version == "HTTP/2.0"


@pytest.mark.asyncio
async def test_http_version_pinned_to_h1_async():
    transport = rqx.AsyncHTTPTransport(http1=True, http2=False)
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(url="https://nghttp2.org/httpbin/get")
    assert resp.http_version == "HTTP/1.1"


@pytest.mark.asyncio
async def test_http_version_pinned_to_h2_prior_knowledge_async():
    transport = rqx.AsyncHTTPTransport(http1=False, http2=True)
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(url="https://nghttp2.org/httpbin/get")
    assert resp.http_version == "HTTP/2.0"


@pytest.mark.asyncio
async def test_http_version_both_disabled_raises_async():
    with pytest.raises(rqx.RqxError):
        rqx.AsyncHTTPTransport(http1=False, http2=False)


def test_proxy_config():
    transport = rqx.AsyncHTTPTransport(proxy={"https": "http://localhost:8080"})
    assert transport is not None


@pytest.mark.asyncio
async def test_verify_is_false_returns_200_on_unsigned_url():
    transport = rqx.AsyncHTTPTransport(verify=False)
    client = rqx.AsyncClient(transport=transport)
    # hitting a normal HTTPS endpoint still works
    resp = await client.get("https://nghttp2.org/httpbin/get")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_cookies_basic():
    client = rqx.AsyncClient()

    # First request sets the cookie
    resp1 = await client.get(f"{HTTPBIN_HOST}/cookies/set/testcookie/hello")
    assert "testcookie" in resp1.cookies
    assert resp1.cookies["testcookie"] == "hello"

    # Client should have the cookie stored
    assert "testcookie" in client.cookies
    assert client.cookies["testcookie"] == "hello"

    # Second request should send the cookie back
    resp2 = await client.get(f"{HTTPBIN_HOST}/cookies")
    body = resp2.json()
    assert body["cookies"]["testcookie"] == "hello"


@pytest.mark.asyncio
async def test_async_stream():
    async with rqx.AsyncClient() as client:
        resp = await client.stream("GET", f"{HTTPBIN_HOST}/stream/5")
        chunks = []
        async for chunk in resp.aiter_bytes(1024):
            chunks.append(chunk)
        assert len(chunks) > 0
