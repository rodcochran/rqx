"""Cookie jar: accumulated across a redirect, scoped to the host that set it."""


def test_cookie_set_during_a_redirect_chain_is_sent_afterwards(lib, flaky_server):
    client = lib.client(follow_redirects=True)
    resp = client.get(f"{flaky_server}/cookies/set?session=abc")
    assert resp.json() == {"cookies": {"session": "abc"}}
    assert client.get(f"{flaky_server}/cookies").json() == {
        "cookies": {"session": "abc"}
    }


def test_cookie_is_not_sent_to_a_different_host(lib, flaky_server):
    """`localhost` and `127.0.0.1` are the same server but different hosts;
    a cookie set by one must not travel to the other."""
    client = lib.client(follow_redirects=True)
    client.get(f"{flaky_server}/cookies/set?session=abc")
    other = flaky_server.replace("localhost", "127.0.0.1")
    assert client.get(f"{other}/cookies").json() == {"cookies": {}}
