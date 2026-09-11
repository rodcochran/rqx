"""Response surface: status, text, json(), classification, final URL."""

import uuid


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
