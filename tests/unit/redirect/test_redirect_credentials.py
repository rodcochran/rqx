"""Credentials across redirects (https://github.com/rodcochran/rqx/issues/205).

As in httpx: a redirect to another origin (scheme, host or port) drops
`Authorization`, and a same-origin redirect keeps it. A hand-set `Cookie` is
dropped on every hop; the client's jar supplies cookies for the next URL.
"""

from dataclasses import dataclass, field
from urllib.parse import quote

import pytest

import rqx

TOKEN = "tok-abc.123"


@dataclass(frozen=True)
class AuthCase:
    name: str
    header: str
    client_kwargs: dict = field(default_factory=dict)
    request_kwargs: dict = field(default_factory=dict)


AUTH_CASES = [
    AuthCase("bearer", f"Bearer {TOKEN}", request_kwargs={"auth_bearer": TOKEN}),
    AuthCase("basic", "Basic dTpw", request_kwargs={"auth": ("u", "p")}),
    AuthCase("client_bearer", f"Bearer {TOKEN}", client_kwargs={"auth_bearer": TOKEN}),
    AuthCase(
        "header",
        f"Bearer {TOKEN}",
        request_kwargs={"headers": {"Authorization": f"Bearer {TOKEN}"}},
    ),
]


@pytest.fixture(params=AUTH_CASES, ids=lambda case: case.name)
def auth(request):
    return request.param


@pytest.fixture(params=["host", "port"])
def other_origin(request, flaky_server, other_port_server):
    """An /echo-auth URL on a different origin from `flaky_server`."""
    base = {
        "host": flaky_server.replace("localhost", "127.0.0.1"),
        "port": other_port_server,
    }[request.param]
    return f"{base}/echo-auth"


def redirect_to(flaky_server, target):
    return f"{flaky_server}/redirect-to?url={quote(target, safe='')}"


COOKIE = {"Cookie": "session=secret"}


# ----- sync -----


def test_cross_origin_redirect_drops_authorization(flaky_server, other_origin, auth):
    client = rqx.Client(follow_redirects=True, **auth.client_kwargs)
    resp = client.get(redirect_to(flaky_server, other_origin), **auth.request_kwargs)
    assert str(resp.url) == other_origin
    assert resp.json()["authorization"] == ""


def test_same_origin_redirect_keeps_authorization(flaky_server, auth):
    client = rqx.Client(follow_redirects=True, **auth.client_kwargs)
    target = f"{flaky_server}/echo-auth"
    resp = client.get(redirect_to(flaky_server, target), **auth.request_kwargs)
    assert resp.json()["authorization"] == auth.header


def test_cross_origin_stream_drops_authorization(flaky_server, other_origin, auth):
    client = rqx.Client(**auth.client_kwargs)
    with client.stream(
        "GET",
        redirect_to(flaky_server, other_origin),
        follow_redirects=True,
        **auth.request_kwargs,
    ) as resp:
        resp.read()
        assert resp.json()["authorization"] == ""


@pytest.mark.parametrize("origin", ["same", "cross"])
def test_redirect_drops_hand_set_cookie(flaky_server, origin):
    base = {
        "same": flaky_server,
        "cross": flaky_server.replace("localhost", "127.0.0.1"),
    }
    target = f"{base[origin]}/cookies"
    resp = rqx.Client(follow_redirects=True).get(
        redirect_to(flaky_server, target), headers=COOKIE
    )
    assert resp.json() == {"cookies": {}}


# ----- async -----


@pytest.mark.asyncio
async def test_cross_origin_redirect_drops_authorization_async(
    flaky_server, other_origin, auth
):
    client = rqx.AsyncClient(follow_redirects=True, **auth.client_kwargs)
    resp = await client.get(
        redirect_to(flaky_server, other_origin), **auth.request_kwargs
    )
    assert resp.json()["authorization"] == ""


@pytest.mark.asyncio
async def test_same_origin_redirect_keeps_authorization_async(flaky_server, auth):
    client = rqx.AsyncClient(follow_redirects=True, **auth.client_kwargs)
    target = f"{flaky_server}/echo-auth"
    resp = await client.get(redirect_to(flaky_server, target), **auth.request_kwargs)
    assert resp.json()["authorization"] == auth.header


@pytest.mark.asyncio
async def test_cross_origin_stream_drops_authorization_async(
    flaky_server, other_origin, auth
):
    client = rqx.AsyncClient(**auth.client_kwargs)
    async with client.stream(
        "GET",
        redirect_to(flaky_server, other_origin),
        follow_redirects=True,
        **auth.request_kwargs,
    ) as resp:
        await resp.aread()
        assert resp.json()["authorization"] == ""


@pytest.mark.asyncio
async def test_cross_origin_redirect_drops_hand_set_cookie_async(flaky_server):
    target = flaky_server.replace("localhost", "127.0.0.1") + "/cookies"
    resp = await rqx.AsyncClient(follow_redirects=True).get(
        redirect_to(flaky_server, target), headers=COOKIE
    )
    assert resp.json() == {"cookies": {}}
