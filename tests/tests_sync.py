import json
import threading
import time

import pytest
import reqx
from rich import print

# HTTPBIN_HOST = "https://httpbin.org"
HTTPBIN_HOST = "http://localhost"


# ================================================================
# Phase 1 tests
# ================================================================


def test_true_200():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/get")
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()
    assert body["url"] == f"{HTTPBIN_HOST}/get"
    print("")
    print(f"JSON body:\n{body}")


def test_400():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/status/400")
    assert resp.status_code == 400
    assert "content-type" in resp.headers
    with pytest.raises(reqx.ReqxError):
        resp.json()


def test_404():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/status/404")
    assert resp.status_code == 404
    assert "content-type" in resp.headers
    with pytest.raises(reqx.ReqxError):
        resp.json()


def test_500():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/status/500")
    assert resp.status_code == 500
    assert "content-type" in resp.headers
    with pytest.raises(reqx.ReqxError):
        resp.json()


def test_body():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/get")
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()

    expected_body_keys = ["args", "headers", "origin", "url"]

    for k in expected_body_keys:
        assert k in body.keys()


def test_valid_text():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/get")
    text = resp.text()
    assert text is not None
    assert isinstance(text, str)
    assert len(text) > 0
    print("")
    print(f"Text:\n{text}")


def test_valid_bytes():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/get")
    content = resp.content
    assert content is not None
    assert isinstance(content, bytes)
    assert not isinstance(content, list)
    print("")
    print(f"Content:\n{content}")


def test_headers():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/get")
    headers = resp.headers
    assert headers is not None
    assert isinstance(headers, dict)
    assert headers["content-type"] == "application/json"
    print("")
    print(f"Headers:\n{headers}")


def test_nested_json():
    # httpbin's /get?foo=bar&baz=123 will give you query params in the args field.
    # Good way to test that nested JSON values come through correctly.
    client = reqx.Client()

    key1 = "baz"
    val1 = "123"
    key2 = "foo"
    val2 = "bar"

    resp = client.get(f"{HTTPBIN_HOST}/get?{key1}={val1}&{key2}={val2}")
    body = resp.json()
    args = body["args"]
    assert args[key1] == val1
    assert args[key2] == val2
    print("")
    print(f"Nested Json (body args):\n{body}")


def test_gil_release():

    def task(wait_time: int):
        print(f"Starting {wait_time} second wait")
        resp = client.get(f"{HTTPBIN_HOST}/delay/{wait_time}")
        print(f"Finished {wait_time} second wait")
        assert resp.status_code == 200
        assert "content-type" in resp.headers

    client = reqx.Client()

    wait_time_1 = 1
    wait_time_2 = 2

    t1 = threading.Thread(target=task, args=(wait_time_1,))
    t2 = threading.Thread(target=task, args=(wait_time_2,))

    start = time.perf_counter()
    t1.start()
    t2.start()

    t1.join()
    t2.join()

    end = time.perf_counter()
    duration = end - start

    print("")
    print(f"Duration: {duration}s")
    assert duration <= max(wait_time_1, wait_time_2) * 1.1


# ================================================================
# Phase 2 tests
# ================================================================


def test_blank_post():
    client = reqx.Client()
    resp = client.post(f"{HTTPBIN_HOST}/post", json=None)
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Post JSON response:\n{body}")


def test_sample_json_params_post():
    client = reqx.Client()
    resp = client.post(f"{HTTPBIN_HOST}/post", json={"special_param": 1})
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Post JSON response:\n{body}")


def test_blank_options():
    client = reqx.Client()
    resp = client.options(f"{HTTPBIN_HOST}/get")
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    assert "allow" in resp.headers


def test_blank_head():
    client = reqx.Client()
    resp = client.head(f"{HTTPBIN_HOST}/get")
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    assert not resp.content


def test_blank_put():
    client = reqx.Client()
    resp = client.put(f"{HTTPBIN_HOST}/put", json=None)
    assert resp.status_code == 200
    assert "content-type" in resp.headers


def test_sample_json_params_put():
    client = reqx.Client()
    resp = client.put(f"{HTTPBIN_HOST}/put", json={"special_param": 1})
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Post JSON response:\n{body}")


def test_blank_patch():
    client = reqx.Client()
    resp = client.patch(f"{HTTPBIN_HOST}/patch", json=None)
    assert resp.status_code == 200
    assert "content-type" in resp.headers


def test_sample_json_params_patch():
    client = reqx.Client()
    resp = client.patch(f"{HTTPBIN_HOST}/patch", json={"special_param": 1})
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Post JSON response:\n{body}")


def test_blank_delete():
    client = reqx.Client()
    resp = client.delete(f"{HTTPBIN_HOST}/delete")
    assert resp.status_code == 200
    assert "content-type" in resp.headers


def test_post_with_query_params():

    query_param_1_key = "q_param_1"
    query_param_1_value = "hey"

    client = reqx.Client()
    resp = client.post(
        f"{HTTPBIN_HOST}/post",
        params={query_param_1_key: query_param_1_value},
    )
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()

    assert body["args"][query_param_1_key] == query_param_1_value

    print("")
    print(f"Post JSON response:\n{body}")


def test_post_with_query_params_and_json():

    query_param_1_key = "q_param_1"
    query_param_1_value = "hey"

    json_param_1_key = "special_param"
    json_param_1_value = 1

    client = reqx.Client()
    resp = client.post(
        f"{HTTPBIN_HOST}/post",
        json={json_param_1_key: json_param_1_value},
        params={query_param_1_key: query_param_1_value},
    )
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Post JSON response:\n{body}")

    assert body["args"][query_param_1_key] == query_param_1_value
    assert body["json"] == {json_param_1_key: json_param_1_value}
    assert json.loads(body["data"]) == {json_param_1_key: json_param_1_value}


def test_put_with_query_params():

    query_param_1_key = "q_param_1"
    query_param_1_value = "hey"

    client = reqx.Client()
    resp = client.put(
        f"{HTTPBIN_HOST}/put",
        params={query_param_1_key: query_param_1_value},
    )
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()

    assert body["args"][query_param_1_key] == query_param_1_value

    print("")
    print(f"Put JSON response:\n{body}")


def test_put_with_query_params_and_json():

    query_param_1_key = "q_param_1"
    query_param_1_value = "hey"

    json_param_1_key = "special_param"
    json_param_1_value = 1

    client = reqx.Client()
    resp = client.put(
        f"{HTTPBIN_HOST}/put",
        json={json_param_1_key: json_param_1_value},
        params={query_param_1_key: query_param_1_value},
    )
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Put JSON response:\n{body}")

    assert body["args"][query_param_1_key] == query_param_1_value
    assert body["json"] == {json_param_1_key: json_param_1_value}
    assert json.loads(body["data"]) == {json_param_1_key: json_param_1_value}


def test_patch_with_query_params():

    query_param_1_key = "q_param_1"
    query_param_1_value = "hey"

    client = reqx.Client()
    resp = client.patch(
        f"{HTTPBIN_HOST}/patch",
        params={query_param_1_key: query_param_1_value},
    )
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()

    assert body["args"][query_param_1_key] == query_param_1_value

    print("")
    print(f"Patch JSON response:\n{body}")


def test_patch_with_query_params_and_json():

    query_param_1_key = "q_param_1"
    query_param_1_value = "hey"

    json_param_1_key = "special_param"
    json_param_1_value = 1

    client = reqx.Client()
    resp = client.patch(
        f"{HTTPBIN_HOST}/patch",
        json={json_param_1_key: json_param_1_value},
        params={query_param_1_key: query_param_1_value},
    )
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    print("")
    print(f"Patch JSON response:\n{body}")

    assert body["args"][query_param_1_key] == query_param_1_value
    assert body["json"] == {json_param_1_key: json_param_1_value}
    assert json.loads(body["data"]) == {json_param_1_key: json_param_1_value}


def test_get_with_headers():

    headers = {
        "Authorization": "Bearer your_access_token_here",
        "X-API-Key": "your_api_key_string",
    }

    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/get", headers=headers)

    print("")
    print(f"Response Headers:\n{resp.headers}")

    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()
    assert body["url"] == f"{HTTPBIN_HOST}/get"

    echoed_headers = body["headers"]

    print(f"Echoed Headers: \n{echoed_headers}")

    for k, v in headers.items():
        lower_k = k.lower()
        for _k, _v in echoed_headers.items():
            _lower_k = _k.lower()
            if lower_k == _lower_k:
                assert echoed_headers[_k] == v
                break

    print("")
    print(f"JSON body:\n{body}")


def test_get_with_timeout():
    client = reqx.Client()
    with pytest.raises(reqx.TimeoutException):
        client.get(f"{HTTPBIN_HOST}/delay/5", timeout=1)


def test_context_manger_200():
    with reqx.Client() as client:
        resp = client.get(f"{HTTPBIN_HOST}/get")
        assert resp.status_code == 200
        assert "content-type" in resp.headers
        body = resp.json()
        assert body["url"] == f"{HTTPBIN_HOST}/get"
        print("")
        print(f"JSON body:\n{body}")


def test_raise_for_status():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/status/400")
    assert resp.status_code == 400
    assert "content-type" in resp.headers
    with pytest.raises(reqx.HTTPStatusError):
        resp.raise_for_status()


def test_post_with_content():
    content_str = '{"raw_content": "hello"}'
    content_bytes = content_str.encode()
    client = reqx.Client()
    resp = client.post(f"{HTTPBIN_HOST}/post", content=content_bytes)
    assert resp.status_code == 200
    assert "content-type" in resp.headers

    body = resp.json()
    assert body["data"] == content_str
    print("")
    print(f"Post JSON response:\n{body}")


def test_post_with_data():
    data = {"hi": "goodbye", "hey": "2"}
    client = reqx.Client()
    resp = client.post(f"{HTTPBIN_HOST}/post", data=data)
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()
    assert body["headers"]["Content-Type"] == "application/x-www-form-urlencoded"
    print("")
    print(f"Post JSON response:\n{body}")


def test_basic_auth():
    u = "User"
    p = "Password"
    auth = (u, p)
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/basic-auth/{u}/{p}", auth=auth)
    assert resp.status_code == 200


def test_basic_client_based_redirect():
    client = reqx.Client(follow_redirects=True)
    resp = client.get(
        f"{HTTPBIN_HOST}/redirect/3",
    )
    assert resp.status_code == 200


def test_basic_request_based_redirect():
    client = reqx.Client(follow_redirects=False)
    resp = client.get(
        f"{HTTPBIN_HOST}/redirect/3",
        follow_redirects=True,
    )
    assert resp.status_code == 200


def test_false_follow_redirects_returns_302():
    client = reqx.Client(follow_redirects=False)
    resp = client.get(
        f"{HTTPBIN_HOST}/redirect/3",
        follow_redirects=False,
    )
    assert resp.status_code == 302


def test_raise_error_on_redirects_exeeding_max_redirects():
    client = reqx.Client(follow_redirects=True, max_redirects=1)
    with pytest.raises(reqx.TooManyRedirects):
        client.get(f"{HTTPBIN_HOST}/redirect/3")


def test_get_total_elapsed_time():
    delay_time = 1
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/delay/{delay_time}")
    assert resp.elapsed is not None
    assert resp.elapsed > delay_time
    print("")
    print(f"Elapsed time:\n{resp.elapsed:.2f}s")


def test_basic_final_url_in_output():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/get")
    assert resp.url == f"{HTTPBIN_HOST}/get"


def test_redirected_final_url_in_output():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/redirect/3", follow_redirects=True)
    assert resp.url == f"{HTTPBIN_HOST}/get"


def test_bad_url_raises():
    client = reqx.Client()
    with pytest.raises(reqx.ReqxError):
        client.get("Bad URL")


# ================================================================
# Phase 3 tests
# ================================================================


def test_retry_init():

    retry = reqx.Retry()

    assert retry is not None

    # base
    assert retry.total is not None

    # dependent retry counts (should inherit from total)
    assert retry.connect is not None
    assert retry.read is not None
    assert retry.status is not None

    # Assert that they inherit from total
    assert retry.connect == retry.total
    assert retry.read == retry.total
    assert retry.status == retry.total

    # backoff config
    assert retry.backoff_factor is not None
    assert retry.backoff_max is not None
    assert retry.backoff_jitter is not None

    # collections
    assert retry.status_forcelist is not None
    assert retry.allowed_methods is not None

    # flags
    assert retry.respect_retry_after_header is not None
    assert retry.raise_on_status is not None
    assert retry.raise_on_redirect is not None


def test_transport_init():

    transport = reqx.HTTPTransport()

    assert transport is not None
    assert transport.retries is None


def test_retry_on_flaky_server(flaky_server):
    transport = reqx.HTTPTransport(
        retries=reqx.Retry(
            total=5,
            backoff_factor=0.1,
            status_forcelist={503},
        )
    )
    client = reqx.Client(transport=transport)
    resp = client.get(f"{flaky_server}/flaky?request_id=test1")
    assert resp.status_code == 200


def test_exceeded_retries_on_flaky_server(flaky_server):
    transport = reqx.HTTPTransport(
        retries=reqx.Retry(
            total=1,
            backoff_factor=0.1,
            status_forcelist={503},
        )
    )
    client = reqx.Client(transport=transport)

    with pytest.raises(reqx.MaxRetriesExceeded):
        client.get(f"{flaky_server}/flaky?request_id=test2")


def test_404_is_not_retried():
    transport = reqx.HTTPTransport(
        retries=reqx.Retry(
            total=1,
            backoff_factor=0.1,
            status_forcelist={503},
        )
    )
    client = reqx.Client(transport=transport)

    resp = client.get(f"{HTTPBIN_HOST}/status/404")
    assert resp.status_code == 404
    assert "content-type" in resp.headers
    with pytest.raises(reqx.ReqxError):
        resp.json()


def test_not_allowed_method_is_not_retried(flaky_server):
    transport = reqx.HTTPTransport(
        retries=reqx.Retry(
            total=1,
            backoff_factor=0.1,
            status_forcelist={503},
            allowed_methods={"POST"},
        )
    )
    client = reqx.Client(transport=transport)
    resp = client.get(f"{flaky_server}/flaky?request_id=test3")

    assert resp.status_code == 503
    assert "content-type" in resp.headers


def test_retry_history_populated(flaky_server):
    transport = reqx.HTTPTransport(
        retries=reqx.Retry(
            total=5,
            backoff_factor=0.1,
            status_forcelist={503},
        )
    )
    client = reqx.Client(transport=transport)
    resp = client.get(f"{flaky_server}/flaky?request_id=test4")
    assert resp.status_code == 200
    assert resp.num_retries == 2
    assert len(resp.retry_history) == 2
    assert resp.retry_history[0][0] == "503"  # status code string
    print("")
    print(f"Retry History:\n{resp.retry_history}")


def test_total_timeout_exceeded(flaky_server):
    transport = reqx.HTTPTransport(
        retries=reqx.Retry(
            total=5,
            backoff_factor=2.0,
            status_forcelist={503},
            total_timeout=1.0,
        )
    )
    client = reqx.Client(transport=transport)
    with pytest.raises(reqx.MaxRetriesExceeded):
        client.get(f"{flaky_server}/flaky?request_id=test5")


def test_total_timeout_not_exceeded(flaky_server):
    transport = reqx.HTTPTransport(
        retries=reqx.Retry(
            total=5,
            backoff_factor=0.1,
            status_forcelist={503},
            total_timeout=30.0,
        )
    )
    client = reqx.Client(transport=transport)
    resp = client.get(f"{flaky_server}/flaky?request_id=test6")
    assert resp.status_code == 200
    assert resp.num_retries == 2
    assert len(resp.retry_history) == 2
    assert resp.retry_history[0][0] == "503"  # status code string
    print("")
    print(f"Retry History:\n{resp.retry_history}")


# ================================================================
# Phase 4 tests
# ================================================================


def test_max_connections_with_freed_gil():

    def task(wait_time: int):
        print(f"Starting {wait_time} second wait")
        resp = client.get(f"{HTTPBIN_HOST}/delay/{wait_time}")
        print(f"Finished {wait_time} second wait")
        assert resp.status_code == 200
        assert "content-type" in resp.headers

    transport = reqx.HTTPTransport(max_connections=2)
    client = reqx.Client(transport=transport)

    wait_time_1 = 1
    wait_time_2 = 1
    wait_time_3 = 1
    wait_time_4 = 1
    wait_time_5 = 1

    wait_times = [wait_time_1, wait_time_2, wait_time_3, wait_time_4, wait_time_5]

    t1 = threading.Thread(target=task, args=(wait_time_1,))
    t2 = threading.Thread(target=task, args=(wait_time_2,))
    t3 = threading.Thread(target=task, args=(wait_time_3,))
    t4 = threading.Thread(target=task, args=(wait_time_4,))
    t5 = threading.Thread(target=task, args=(wait_time_5,))

    start = time.perf_counter()
    t1.start()
    t2.start()
    t3.start()
    t4.start()
    t5.start()

    t1.join()
    t2.join()
    t3.join()
    t4.join()
    t5.join()

    end = time.perf_counter()
    duration = end - start

    print("")
    print(f"Duration: {duration}s")

    all_parallel_time = max(wait_times)
    all_serial_time = sum(wait_times)

    assert all_parallel_time < duration < all_serial_time

    print(
        f"All Parallel Time: {all_parallel_time}s\n"
        f"Duration: {duration}s\n"
        f"All Serial: {all_serial_time}s"
    )


def test_basic_http2():
    transport = reqx.HTTPTransport(http2=True)
    client = reqx.Client(transport=transport)
    url = "https://nghttp2.org/httpbin/get"
    resp = client.get(url=url)
    assert resp.http_version == "HTTP/2.0"


def test_basic_http2_explicit_opt_out():
    transport = reqx.HTTPTransport(http2=False)
    client = reqx.Client(transport=transport)
    url = "https://nghttp2.org/httpbin/get"
    resp = client.get(url=url)
    assert resp.http_version != "HTTP/2.0"


def test_basic_http2_implicit_opt_out():
    transport = reqx.HTTPTransport()
    client = reqx.Client(transport=transport)
    url = "https://nghttp2.org/httpbin/get"
    resp = client.get(url=url)
    assert resp.http_version != "HTTP/2.0"
