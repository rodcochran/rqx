"""Tests for the specific exception types raised on different failure modes (Issue #1)."""

import pytest

import rqx


def test_connect_error_dns_failure():
    """A DNS failure raises ConnectError (subclass of NetworkError, RqxError)."""
    client = rqx.Client()
    with pytest.raises(rqx.ConnectError):
        client.get("http://nonexistent.invalid.example.com/")


def test_connect_error_connection_refused():
    """A closed port raises ConnectError."""
    client = rqx.Client()
    with pytest.raises(rqx.ConnectError):
        client.get("http://127.0.0.1:1/")  # port 1 is reserved, nothing listens


def test_connect_error_is_subclass_of_rqxerror():
    """Hierarchy invariant: ConnectError → NetworkError → TransportError → RequestError → RqxError."""
    assert issubclass(rqx.ConnectError, rqx.NetworkError)
    assert issubclass(rqx.NetworkError, rqx.TransportError)
    assert issubclass(rqx.TransportError, rqx.RequestError)
    assert issubclass(rqx.RequestError, rqx.RqxError)


def test_connect_error_caught_by_rqxerror():
    """Old-style except rqx.RqxError still catches new-style ConnectError."""
    client = rqx.Client()
    with pytest.raises(rqx.RqxError):
        client.get("http://nonexistent.invalid.example.com/")


def test_read_timeout(flaky_server):
    """Server takes longer than the client's timeout — raises ReadTimeout."""
    client = rqx.Client(timeout=1)  # 1-second timeout
    with pytest.raises(rqx.ReadTimeout):
        client.get(f"{flaky_server}/sleep/3")  # server sleeps 3s


def test_read_timeout_caught_by_timeout_exception(flaky_server):
    """ReadTimeout is catchable as TimeoutException."""
    client = rqx.Client(timeout=1)
    with pytest.raises(rqx.TimeoutException):
        client.get(f"{flaky_server}/sleep/3")


def test_too_many_redirects(flaky_server):
    """Redirect loop raises TooManyRedirects."""
    client = rqx.Client(follow_redirects=True, max_redirects=3)
    with pytest.raises(rqx.TooManyRedirects):
        client.get(f"{flaky_server}/redirect-loop")


def test_too_many_redirects_caught_by_rqxerror(flaky_server):
    """TooManyRedirects is catchable as RqxError."""
    client = rqx.Client(follow_redirects=True, max_redirects=3)
    with pytest.raises(rqx.RqxError):
        client.get(f"{flaky_server}/redirect-loop")


def test_read_error_on_mid_response_close(flaky_server):
    """Server closes mid-response — should surface as a NetworkError-flavored error.

    The /reset endpoint accepts the connection then closes immediately without
    sending anything. Reqwest reports this as a connect error since no response
    was received. The exact mapping depends on whether the kernel saw bytes
    flow or not; ConnectError is the most defensible classification.
    """
    client = rqx.Client()
    # No retries — we want to see the underlying error type.
    with pytest.raises(rqx.RqxError):
        client.get(f"{flaky_server}/reset?request_id=read_error_test")


# ----- async variants for the most important cases -----


@pytest.mark.asyncio
async def test_connect_error_dns_failure_async():
    client = rqx.AsyncClient()
    with pytest.raises(rqx.ConnectError):
        await client.get("http://nonexistent.invalid.example.com/")


@pytest.mark.asyncio
async def test_read_timeout_async(flaky_server):
    client = rqx.AsyncClient(timeout=1)
    with pytest.raises(rqx.ReadTimeout):
        await client.get(f"{flaky_server}/sleep/3")


@pytest.mark.asyncio
async def test_too_many_redirects_async(flaky_server):
    client = rqx.AsyncClient(follow_redirects=True, max_redirects=3)
    with pytest.raises(rqx.TooManyRedirects):
        await client.get(f"{flaky_server}/redirect-loop")
