"""Bodies across redirects and retries (https://github.com/rodcochran/rqx/issues/149).
/redirect/<status> -> redirect to /echo-body
/echo-body         -> {"method", "content_type", "body"}
/flaky-echo-body   -> 503 twice, then echo
"""

from urllib.parse import parse_qs

import pytest

import rqx

PAYLOAD = {"id": 1}
# POST is not retried by default (not idempotent); opt it in for the retry tests.
RETRY_POST = rqx.Retry(
    total=5, backoff_factor=0.0, status_forcelist={503}, allowed_methods={"POST"}
)


def _client():
    return rqx.Client(follow_redirects=True)


@pytest.mark.parametrize("status", [307, 308])
def test_method_and_body_survive_redirect(flaky_server, status):
    resp = _client().post(f"{flaky_server}/redirect/{status}", json=PAYLOAD)
    echo = resp.json()
    assert resp.status_code == 200
    assert echo["method"] == "POST"
    assert echo["content_type"] == "application/json"
    assert echo["body"] == '{"id":1}'


@pytest.mark.parametrize("status", [302, 303])
def test_downgrade_to_get_drops_body_and_its_headers(flaky_server, status):
    resp = _client().post(f"{flaky_server}/redirect/{status}", json=PAYLOAD)
    echo = resp.json()
    assert echo["method"] == "GET"
    assert echo["content_type"] is None
    assert echo["body"] == ""


def test_form_body_survives_307(flaky_server):
    resp = _client().post(f"{flaky_server}/redirect/307", data={"a": "1", "b": "2"})
    echo = resp.json()
    assert echo["method"] == "POST"
    assert echo["content_type"] == "application/x-www-form-urlencoded"
    assert parse_qs(echo["body"]) == {"a": ["1"], "b": ["2"]}


def test_raw_content_survives_308(flaky_server):
    resp = _client().put(f"{flaky_server}/redirect/308", content=b"raw bytes")
    echo = resp.json()
    assert echo["method"] == "PUT"
    assert echo["body"] == "raw bytes"


def test_301_downgrades_post_to_get_like_httpx(flaky_server):
    """301 and 302 turn a POST into a body-less GET, as httpx and browsers do
    (https://github.com/rodcochran/rqx/issues/42). Other methods are kept."""
    resp = _client().post(f"{flaky_server}/redirect/301", json=PAYLOAD)
    echo = resp.json()
    assert echo["method"] == "GET"
    assert echo["body"] == ""


def test_retried_request_carries_its_body(flaky_server):
    transport = rqx.HTTPTransport(retries=RETRY_POST)
    client = rqx.Client(transport=transport)
    resp = client.post(
        f"{flaky_server}/flaky-echo-body?request_id=retry_body_sync", json=PAYLOAD
    )
    assert resp.status_code == 200
    assert resp.num_retries == 2
    assert resp.json()["body"] == '{"id":1}'


def test_relative_location_resolves_against_current_hop(flaky_server):
    resp = _client().get(f"{flaky_server}/nested/hop1")
    assert resp.status_code == 200
    assert resp.text == "final"
    assert resp.url.endswith("/nested/final")


@pytest.mark.asyncio
async def test_method_and_body_survive_307_async(flaky_server):
    client = rqx.AsyncClient(follow_redirects=True)
    resp = await client.post(f"{flaky_server}/redirect/307", json=PAYLOAD)
    echo = resp.json()
    assert echo["method"] == "POST"
    assert echo["body"] == '{"id":1}'


@pytest.mark.asyncio
async def test_downgrade_to_get_drops_body_async(flaky_server):
    client = rqx.AsyncClient(follow_redirects=True)
    resp = await client.post(f"{flaky_server}/redirect/303", json=PAYLOAD)
    echo = resp.json()
    assert echo["method"] == "GET"
    assert echo["body"] == ""


@pytest.mark.asyncio
async def test_retried_request_carries_its_body_async(flaky_server):
    transport = rqx.AsyncHTTPTransport(retries=RETRY_POST)
    client = rqx.AsyncClient(transport=transport)
    resp = await client.post(
        f"{flaky_server}/flaky-echo-body?request_id=retry_body_async", json=PAYLOAD
    )
    assert resp.num_retries == 2
    assert resp.json()["body"] == '{"id":1}'
