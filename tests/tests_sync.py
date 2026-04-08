import reqx
from rich import print


def test_true_200():
    client = reqx.Client()
    resp = client.get("https://httpbin.org/get")
    assert resp.status_code == 200
    assert "content-type" in resp.headers
    body = resp.json()
    assert body["url"] == "https://httpbin.org/get"
    print(body)


def test_400():
    client = reqx.Client()
    resp = client.get("https://httpbin.org/status/400")
    assert resp.status_code == 400
    assert "content-type" in resp.headers
    body = None
    try:
        body = resp.json()
    except Exception as e:
        assert "invalid JSON response" in str(e)
    assert not body
