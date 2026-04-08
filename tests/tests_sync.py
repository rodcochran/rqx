import reqx


def test_true_200():
    client = reqx.Client()
    resp = client.get("https://httpbin.org/get")
    assert resp.status_code == 200
    assert "Content-Type" in resp.headers
    body = resp.json()
    assert body["url"] == "https://httpbin.org/get"
