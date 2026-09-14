"""Response surface: status, text, json(), classification, final URL."""

import json
import uuid

import pytest


def test_status_text_and_json_on_a_plain_200(lib, flaky_server):
    resp = lib.client().get(f"{flaky_server}/streamable")
    assert resp.status_code == 200
    assert resp.text == '{"streamed": true}'
    assert resp.json() == {"streamed": True}
    assert resp.is_success
    assert not resp.is_redirect


def test_json_round_trips_a_posted_document(lib, flaky_server):
    payload = {"a": [1, 2.5, "x", None, True], "b": {"nested": "ü"}}
    resp = lib.client().post(f"{flaky_server}/reflect-json", json=payload)
    assert resp.status_code == 200
    assert resp.json() == payload


def test_unfollowed_redirect_is_a_redirect(lib, flaky_server):
    resp = lib.client(follow_redirects=False).get(f"{flaky_server}/redirect-once")
    assert resp.status_code == 302
    assert resp.is_redirect
    assert not resp.is_success
    assert resp.headers["location"] == "/streamable"


def test_client_error_classification(lib, flaky_server):
    resp = lib.client().delete(f"{flaky_server}/no-such-route")
    assert resp.status_code == 404
    assert resp.is_client_error
    assert not resp.is_success


def test_server_error_classification(lib, flaky_server):
    resp = lib.client().get(f"{flaky_server}/flaky?request_id={uuid.uuid4()}")
    assert resp.status_code == 503
    assert resp.is_server_error
    assert not resp.is_success


def test_final_url_after_following_a_redirect(lib, flaky_server):
    resp = lib.client(follow_redirects=True).get(f"{flaky_server}/redirect-once")
    assert resp.status_code == 200
    assert str(resp.url) == f"{flaky_server}/streamable"


# ----- https://github.com/rodcochran/rqx/issues/189 -----


def test_raise_for_status_returns_the_response(lib, flaky_server):
    resp = lib.client().get(f"{flaky_server}/streamable")
    assert resp.raise_for_status() is resp


def test_status_error_carries_the_response(lib, flaky_server):
    resp = lib.client().delete(f"{flaky_server}/no-such-route")
    with pytest.raises(lib.module.HTTPStatusError) as caught:
        resp.raise_for_status()
    assert caught.value.response.status_code == 404


def test_elapsed_is_a_timedelta(lib, flaky_server):
    resp = lib.client().get(f"{flaky_server}/streamable")
    assert resp.elapsed.total_seconds() >= 0


def test_invalid_json_raises_json_decode_error(lib, canned_server):
    body = b"<html>"
    head = f"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {len(body)}\r\n\r\n"
    resp = lib.client().get(canned_server(head.encode() + body))
    with pytest.raises(json.JSONDecodeError):
        resp.json()


@pytest.mark.parametrize(
    ("method", "path"), [("DELETE", "/no-such-route"), ("GET", "/redirect-once")]
)
def test_status_error_message_matches(lib, flaky_server, method, path):
    """Same text as httpx, including the redirect location and the MDN line."""
    resp = lib.client().request(method, f"{flaky_server}{path}")
    with pytest.raises(lib.module.HTTPStatusError) as caught:
        resp.raise_for_status()
    expected = {
        "/no-such-route": (
            f"Client error '404 Not Found' for url '{flaky_server}/no-such-route'\n"
            "For more information check: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/404"
        ),
        "/redirect-once": (
            f"Redirect response '302 Found' for url '{flaky_server}/redirect-once'\n"
            "Redirect location: '/streamable'\n"
            "For more information check: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/302"
        ),
    }[path]
    assert str(caught.value) == expected
