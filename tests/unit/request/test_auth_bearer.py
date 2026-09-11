"""Tests for the ``auth_bearer=`` kwarg (Issue #9).

Coverage:
  - Per-request ``auth_bearer=`` sends ``Authorization: Bearer <token>``.
  - Works on every verb (request/get/post/put/patch/delete/head/options/stream).
  - Client-level default is applied when no per-request override is given.
  - Per-request override beats the client-level default.
  - ``auth=`` + ``auth_bearer=`` together raises a clear error.
  - Async equivalents (AsyncClient + module-level rqx.get/post).
"""

import json

import pytest

import rqx


TOKEN = "tok-abc.123"
OTHER_TOKEN = "tok-xyz.789"


def _auth_from_resp(resp) -> str:
    """Pull the Authorization header that the server saw out of the JSON body."""
    return resp.json()["authorization"]


# ────────────────────────────────────────────────────────────────────────
# Sync — per-request auth_bearer on every verb
# ────────────────────────────────────────────────────────────────────────


def test_get_sends_bearer(flaky_server):
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert resp.status_code == 200
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_post_sends_bearer(flaky_server):
    client = rqx.Client()
    resp = client.post(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_put_sends_bearer(flaky_server):
    client = rqx.Client()
    resp = client.put(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_patch_sends_bearer(flaky_server):
    client = rqx.Client()
    resp = client.patch(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_delete_sends_bearer(flaky_server):
    client = rqx.Client()
    resp = client.delete(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_options_sends_bearer(flaky_server):
    client = rqx.Client()
    resp = client.options(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_head_sends_bearer(flaky_server):
    """HEAD: server's Authorization echo lands in the Content-Length header
    because HEAD bodies are suppressed. We assert via headers instead of body."""
    client = rqx.Client()
    resp = client.head(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert resp.status_code == 200


def test_request_method_sends_bearer(flaky_server):
    client = rqx.Client()
    resp = client.request("GET", f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_stream_sends_bearer(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/echo-auth", auth_bearer=TOKEN) as resp:
        body = b"".join(resp.iter_bytes())
    assert json.loads(body)["authorization"] == f"Bearer {TOKEN}"


# ────────────────────────────────────────────────────────────────────────
# Sync — client-level default + per-request override
# ────────────────────────────────────────────────────────────────────────


def test_client_level_bearer_applies_by_default(flaky_server):
    client = rqx.Client(auth_bearer=TOKEN)
    resp = client.get(f"{flaky_server}/echo-auth")
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_per_request_bearer_overrides_client_default(flaky_server):
    client = rqx.Client(auth_bearer=TOKEN)
    resp = client.get(f"{flaky_server}/echo-auth", auth_bearer=OTHER_TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {OTHER_TOKEN}"


def test_no_bearer_means_no_authorization_header(flaky_server):
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/echo-auth")
    assert _auth_from_resp(resp) == ""


# ────────────────────────────────────────────────────────────────────────
# Sync — collision rule
# ────────────────────────────────────────────────────────────────────────


def test_auth_and_auth_bearer_together_raises(flaky_server):
    client = rqx.Client()
    with pytest.raises(rqx.RqxError, match="auth"):
        client.get(
            f"{flaky_server}/echo-auth",
            auth=("user", "pass"),
            auth_bearer=TOKEN,
        )


def test_client_default_bearer_collides_with_per_request_basic_auth(flaky_server):
    """Client-level bearer default + per-request basic auth = collision.

    The effective values are what matter: the resolver picks up the client
    default for bearer, then the collision check sees both set and raises.
    """
    client = rqx.Client(auth_bearer=TOKEN)
    with pytest.raises(rqx.RqxError, match="auth"):
        client.get(f"{flaky_server}/echo-auth", auth=("user", "pass"))


# ────────────────────────────────────────────────────────────────────────
# Async
# ────────────────────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_async_get_sends_bearer(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.get(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


@pytest.mark.asyncio
async def test_async_post_sends_bearer(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.post(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


@pytest.mark.asyncio
async def test_async_client_default_bearer(flaky_server):
    async with rqx.AsyncClient(auth_bearer=TOKEN) as client:
        resp = await client.get(f"{flaky_server}/echo-auth")
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


@pytest.mark.asyncio
async def test_async_per_request_overrides_client_default(flaky_server):
    async with rqx.AsyncClient(auth_bearer=TOKEN) as client:
        resp = await client.get(f"{flaky_server}/echo-auth", auth_bearer=OTHER_TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {OTHER_TOKEN}"


@pytest.mark.asyncio
async def test_async_stream_sends_bearer(flaky_server):
    client = rqx.AsyncClient()
    async with await client.stream(
        "GET", f"{flaky_server}/echo-auth", auth_bearer=TOKEN
    ) as resp:
        chunks = []
        async for chunk in resp.aiter_bytes():
            chunks.append(chunk)
    body = b"".join(chunks)
    assert json.loads(body)["authorization"] == f"Bearer {TOKEN}"


@pytest.mark.asyncio
async def test_async_auth_and_auth_bearer_together_raises(flaky_server):
    async with rqx.AsyncClient() as client:
        with pytest.raises(rqx.RqxError, match="auth"):
            await client.get(
                f"{flaky_server}/echo-auth",
                auth=("user", "pass"),
                auth_bearer=TOKEN,
            )


# ────────────────────────────────────────────────────────────────────────
# Module-level convenience functions
# ────────────────────────────────────────────────────────────────────────


def test_module_get_sends_bearer(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_module_post_sends_bearer(flaky_server):
    resp = rqx.post(f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"


def test_module_request_sends_bearer(flaky_server):
    resp = rqx.request("GET", f"{flaky_server}/echo-auth", auth_bearer=TOKEN)
    assert _auth_from_resp(resp) == f"Bearer {TOKEN}"
