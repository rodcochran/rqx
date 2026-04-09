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
    with pytest.raises(RuntimeError):
        resp.json()


def test_404():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/status/404")
    assert resp.status_code == 404
    assert "content-type" in resp.headers
    with pytest.raises(RuntimeError):
        resp.json()


def test_500():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/status/500")
    assert resp.status_code == 500
    assert "content-type" in resp.headers
    with pytest.raises(RuntimeError):
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
