"""Boolean status-classification properties on responses (Issue #8).

Mirrors httpx:
  - is_informational (1xx)
  - is_success (2xx)
  - is_redirect (3xx AND has Location header)
  - is_client_error (4xx)
  - is_server_error (5xx)
  - is_error (4xx or 5xx)

Covers PyResponse plus both stream-response classes for parity.
"""

import pytest

import rqx


# ---------------------------------------------------------------------------
# 2xx — is_success
# ---------------------------------------------------------------------------


def test_is_success_true_on_200(flaky_server):
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/streamable")
    assert resp.is_success is True
    assert resp.is_informational is False
    assert resp.is_redirect is False
    assert resp.is_client_error is False
    assert resp.is_server_error is False
    assert resp.is_error is False


# ---------------------------------------------------------------------------
# 3xx — is_redirect requires Location header
# ---------------------------------------------------------------------------


def test_is_redirect_true_when_location_present(flaky_server):
    """302 with Location → is_redirect True (the followable case)."""
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/redirect-once")  # follow_redirects default False
    assert resp.status_code == 302
    assert resp.is_redirect is True
    assert resp.is_success is False
    assert resp.is_error is False


def test_is_redirect_false_on_304_without_location(flaky_server):
    """A 3xx that carries no Location header is not classified as a redirect.

    This is the httpx semantic that distinguishes is_redirect (the response
    can be followed) from just-being-a-3xx-status. 304 Not Modified is the
    canonical example.
    """
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/not-modified")
    assert resp.status_code == 304
    assert resp.is_redirect is False
    assert resp.is_success is False
    assert resp.is_client_error is False


# ---------------------------------------------------------------------------
# 4xx — is_client_error / is_error
# ---------------------------------------------------------------------------


def test_is_client_error_on_404(flaky_server):
    """POST to /something on flaky server returns 404."""
    client = rqx.Client()
    resp = client.post(f"{flaky_server}/unknown?request_id=props-test")
    assert resp.status_code == 404
    assert resp.is_client_error is True
    assert resp.is_error is True
    assert resp.is_server_error is False
    assert resp.is_success is False


# ---------------------------------------------------------------------------
# 5xx — is_server_error / is_error
# ---------------------------------------------------------------------------


def test_is_server_error_on_503(flaky_server):
    """First call to flaky server returns 503."""
    client = rqx.Client()
    resp = client.get(f"{flaky_server}/something?request_id=server-error-test")
    assert resp.status_code == 503
    assert resp.is_server_error is True
    assert resp.is_error is True
    assert resp.is_client_error is False


# ---------------------------------------------------------------------------
# Stream response parity
# ---------------------------------------------------------------------------


def test_stream_response_has_status_props(flaky_server):
    """PyStreamResponse exposes the same boolean props as PyResponse."""
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.is_success is True
        assert resp.is_error is False
        assert resp.is_redirect is False


def test_stream_response_is_redirect_with_location(flaky_server):
    client = rqx.Client()
    with client.stream("GET", f"{flaky_server}/redirect-once") as resp:
        assert resp.status_code == 302
        assert resp.is_redirect is True


@pytest.mark.asyncio
async def test_async_stream_response_has_status_props(flaky_server):
    """PyAsyncStreamResponse exposes the same boolean props."""
    client = rqx.AsyncClient()
    async with await client.stream("GET", f"{flaky_server}/streamable") as resp:
        assert resp.is_success is True
        assert resp.is_redirect is False
        assert resp.is_error is False
