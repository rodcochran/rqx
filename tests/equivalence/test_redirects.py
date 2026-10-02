"""Redirect semantics on every standard status, method and body included."""

from urllib.parse import quote

import pytest

ISSUE_116 = "https://github.com/rodcochran/rqx/issues/116"
PAYLOAD = {"k": 1}


@pytest.mark.parametrize(
    "status,expected_method,expected_body",
    [
        (301, "GET", ""),
        (302, "GET", ""),
        (303, "GET", ""),
        (307, "POST", '{"k":1}'),
        (308, "POST", '{"k":1}'),
    ],
)
def test_post_through_each_redirect_status(
    lib, flaky_server, status, expected_method, expected_body
):
    resp = lib.client(follow_redirects=True).post(
        f"{flaky_server}/redirect/{status}", json=PAYLOAD
    )
    echo = resp.json()
    assert resp.status_code == 200
    assert str(resp.url).endswith("/echo-body")
    assert (echo["method"], echo["body"]) == (expected_method, expected_body)


def test_put_survives_a_301(lib, flaky_server):
    """301 downgrades only POST; other methods are kept, like browsers."""
    resp = lib.client(follow_redirects=True).put(
        f"{flaky_server}/redirect/301", json=PAYLOAD
    )
    echo = resp.json()
    assert (echo["method"], echo["body"]) == ("PUT", '{"k":1}')


def test_302_downgrades_every_method_but_head(lib, flaky_server):
    resp = lib.client(follow_redirects=True).put(
        f"{flaky_server}/redirect/302", json=PAYLOAD
    )
    echo = resp.json()
    assert (echo["method"], echo["body"]) == ("GET", "")


def test_303_downgrades_every_method_but_head(lib, flaky_server):
    resp = lib.client(follow_redirects=True).put(
        f"{flaky_server}/redirect/303", json=PAYLOAD
    )
    echo = resp.json()
    assert (echo["method"], echo["body"]) == ("GET", "")


def test_relative_location_resolves_against_the_current_hop(lib, flaky_server):
    resp = lib.client(follow_redirects=True).get(f"{flaky_server}/nested/hop1")
    assert resp.status_code == 200
    assert str(resp.url) == f"{flaky_server}/nested/final"


def test_params_apply_to_the_first_request_only(lib, flaky_server):
    resp = lib.client(follow_redirects=True).get(
        f"{flaky_server}/redirect-once", params={"a": "1"}
    )
    assert str(resp.url) == f"{flaky_server}/streamable"


def test_location_query_survives_params_on_the_first_request(lib, flaky_server):
    target = f"{flaky_server}/echo-url/final?keep=1"
    resp = lib.client(follow_redirects=True).get(
        f"{flaky_server}/redirect-to", params={"url": target}
    )
    assert str(resp.url) == target


def test_redirect_loop_raises_too_many_redirects(lib, flaky_server):
    with pytest.raises(lib.module.TooManyRedirects):
        lib.client(follow_redirects=True, max_redirects=3).get(
            f"{flaky_server}/redirect-loop"
        )


@pytest.mark.rqx_diverges(
    issue=ISSUE_116,
    reason="rqx drops Content-Type along with the body on a 301/302/303 downgrade; httpx keeps the header",
)
def test_content_type_survives_a_downgraded_redirect(lib, flaky_server):
    """httpx strips Content-Length and Transfer-Encoding when the method changes
    but leaves Content-Type on the body-less GET. rqx drops it with the body,
    which says nothing a receiver can use once there is no body."""
    resp = lib.client(follow_redirects=True).post(
        f"{flaky_server}/redirect/301", json=PAYLOAD
    )
    assert resp.json()["content_type"] == "application/json"


def test_cross_origin_redirect_drops_authorization(lib, flaky_server):
    """https://github.com/rodcochran/rqx/issues/205"""
    target = flaky_server.replace("localhost", "127.0.0.1") + "/echo-auth"
    resp = lib.client(follow_redirects=True).get(
        f"{flaky_server}/redirect-to?url={quote(target, safe='')}",
        headers={"Authorization": "Bearer tok"},
    )
    assert str(resp.url) == target
    assert resp.json()["authorization"] == ""


def test_same_origin_redirect_keeps_authorization(lib, flaky_server):
    target = f"{flaky_server}/echo-auth"
    resp = lib.client(follow_redirects=True).get(
        f"{flaky_server}/redirect-to?url={quote(target, safe='')}",
        headers={"Authorization": "Bearer tok"},
    )
    assert resp.json()["authorization"] == "Bearer tok"


@pytest.mark.parametrize("origin", ["same", "cross"])
def test_redirect_drops_hand_set_cookie(lib, flaky_server, origin):
    """The jar, not the first request's header, decides the next hop's cookies."""
    base = {
        "same": flaky_server,
        "cross": flaky_server.replace("localhost", "127.0.0.1"),
    }
    target = f"{base[origin]}/cookies"
    resp = lib.client(follow_redirects=True).get(
        f"{flaky_server}/redirect-to?url={quote(target, safe='')}",
        headers={"Cookie": "session=secret"},
    )
    assert resp.json() == {"cookies": {}}
