"""Tests for Client(base_url=...) / AsyncClient(base_url=...) (Issue #18).

Verifies httpx-style merge semantics:
  - base_url gets a trailing slash auto-appended at construction.
  - Relative request paths have any leading slash stripped, then are joined
    against base_url. This preserves the base's path segments instead of
    dropping them per strict RFC 3986 resolution.
  - Absolute URLs passed to request methods override base_url entirely.
"""

import pytest
import rqx

# ---------------------------------------------------------------------------
# Construction + getter
# ---------------------------------------------------------------------------


def test_base_url_canonicalizes_trailing_slash():
    """A base_url without a trailing slash gets one auto-appended."""
    client = rqx.Client(base_url="https://api.example.com/v1")
    assert client.base_url == "https://api.example.com/v1/"


def test_base_url_preserves_existing_trailing_slash():
    client = rqx.Client(base_url="https://api.example.com/v1/")
    assert client.base_url == "https://api.example.com/v1/"


def test_base_url_default_is_none():
    client = rqx.Client()
    assert client.base_url is None


def test_base_url_malformed_raises():
    with pytest.raises(ValueError):
        rqx.Client(base_url="not a url")


def test_base_url_works_on_async_client():
    client = rqx.AsyncClient(base_url="https://api.example.com/v1")
    assert client.base_url == "https://api.example.com/v1/"


# ---------------------------------------------------------------------------
# Request-time URL resolution
# ---------------------------------------------------------------------------


def test_relative_path_with_leading_slash(flaky_server):
    """The headline case: base_url + "/streamable" → <base>/streamable.

    Verifies the leading slash on the relative path is stripped so it doesn't
    blow away the base's path segments per strict RFC 3986.
    """
    client = rqx.Client(base_url=flaky_server)
    resp = client.get("/streamable")
    assert resp.status_code == 200


def test_relative_path_without_leading_slash(flaky_server):
    """Same destination, no leading slash on the relative path."""
    client = rqx.Client(base_url=flaky_server)
    resp = client.get("streamable")
    assert resp.status_code == 200


def test_absolute_url_overrides_base_url(flaky_server):
    """If the user passes an absolute URL, base_url is ignored entirely."""
    # base_url points somewhere wrong; absolute URL points to the real server.
    client = rqx.Client(base_url="https://wrong.invalid.example.com/")
    resp = client.get(f"{flaky_server}/streamable")
    assert resp.status_code == 200


def test_base_url_with_path_prefix_is_preserved(flaky_server):
    """When base_url has its own path prefix, joining must preserve it.

    `/nested/final` is a real route, so a base of `/nested` plus `/final`
    proves the prefix survived the join instead of being dropped.
    """
    client = rqx.Client(base_url=f"{flaky_server}/nested")
    resp = client.get("/final")
    assert resp.status_code == 200
    assert resp.url.endswith("/nested/final")


def test_relative_path_with_query_string(flaky_server):
    """Query strings on the relative path are preserved through the join."""
    client = rqx.Client(base_url=flaky_server)
    resp = client.get("/streamable?foo=bar")
    assert resp.status_code == 200
    assert "foo=bar" in resp.url


def test_relative_path_combines_with_params_kwarg(flaky_server):
    """A relative path joined to base_url still gets `params=` appended."""
    client = rqx.Client(base_url=flaky_server)
    resp = client.get("/streamable", params={"foo": "bar"})
    assert resp.status_code == 200
    assert "foo=bar" in resp.url


# ---------------------------------------------------------------------------
# Async parity
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_async_relative_path_leading_slash(flaky_server):
    client = rqx.AsyncClient(base_url=flaky_server)
    resp = await client.get("/streamable")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_async_absolute_url_overrides_base_url(flaky_server):
    client = rqx.AsyncClient(base_url="https://wrong.invalid.example.com/")
    resp = await client.get(f"{flaky_server}/streamable")
    assert resp.status_code == 200


# ---------------------------------------------------------------------------
# No base_url + relative path still fails like before (no surprise regression)
# ---------------------------------------------------------------------------


def test_no_base_url_relative_path_is_unsupported_protocol():
    """Without base_url set, a bare relative path has no scheme to send with."""
    client = rqx.Client()
    with pytest.raises(rqx.UnsupportedProtocol):
        client.get("/users")
