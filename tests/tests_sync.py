import reqx
from rich import print

HTTPBIN_HOST = "https://httpbin.org"
# HTTPBIN_HOST = "http://localhost"
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
