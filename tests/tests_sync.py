import threading
import time

import reqx
from rich import print

# HTTPBIN_HOST = "https://httpbin.org"
HTTPBIN_HOST = "http://localhost"
HTTP_BIN_PORT = "80"


def test_true_200():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/get")
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()
    assert body["url"] == f"{HTTPBIN_HOST}/get"
    print(body)


def test_400():
    client = reqx.Client()
    resp = client.get(f"{HTTPBIN_HOST}/status/400")
    assert resp.status_code == 400
    assert "content-type" in resp.headers
    body = None
    try:
        body = resp.json()
    except Exception as e:
        assert "invalid JSON response" in str(e)
    assert not body


def test_gil_release():

    def task(wait_time: int):
        print(f"Starting {wait_time} second wait")
        resp = client.get(f"{HTTPBIN_HOST}/delay/{wait_time}")
        print(f"Finished {wait_time} second wait")
        assert resp.status_code == 200
        assert "content-type" in resp.headers

    client = reqx.Client()

    wait_time_1 = 2
    wait_time_2 = 3

    t1 = threading.Thread(target=task, args=(wait_time_1,))
    t2 = threading.Thread(target=task, args=(wait_time_2,))

    start = time.perf_counter()
    t1.start()
    t2.start()

    t1.join()
    t2.join()

    end = time.perf_counter()
    duration = end - start

    print(f"Duration: {duration}s")
    assert duration <= max(wait_time_1, wait_time_2) * 1.1
