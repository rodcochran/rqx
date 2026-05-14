"""Tests for retry config fields that affect runtime behavior (Issue #2).

Covers:
- backoff_jitter — randomizes backoff timing
- raise_on_status — toggle raise vs return on retry exhaustion
- raise_on_redirect — toggle raise vs return on redirect-loop exhaustion
"""

import time

import pytest

import rqx


# ----- backoff_jitter -----


def test_backoff_jitter_varies_timing(flaky_server):
    """With jitter, repeated retry cycles have different durations."""
    # status_forcelist={503} makes the retry loop actually retry on the
    # flaky server's failures (otherwise only network errors retry).
    retries = rqx.Retry(
        total=3,
        backoff_factor=0.1,
        backoff_jitter=0.5,
        status_forcelist={503},
    )
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport)

    timings = []
    for i in range(3):
        start = time.perf_counter()
        client.get(f"{flaky_server}/?request_id=jitter_test_{i}")
        timings.append(time.perf_counter() - start)

    # jitter=0.5 means backoffs vary ±50% of deterministic value.
    # Three samples should not all be identical at ms resolution.
    assert len(set(round(t, 3) for t in timings)) > 1, (
        f"Expected jittered backoffs to vary across runs, got: {timings}"
    )


def test_backoff_no_jitter_is_deterministic(flaky_server):
    """Without jitter (default), the backoff code path still works."""
    retries = rqx.Retry(
        total=3,
        backoff_factor=0.05,
        backoff_jitter=0.0,
        status_forcelist={503},
    )
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport)

    # Should retry through the flaky failures and succeed.
    resp = client.get(f"{flaky_server}/?request_id=no_jitter_test")
    assert resp.status_code == 200


# ----- raise_on_status -----


def test_raise_on_status_true_raises_on_exhausted_retries(flaky_server):
    """Default: raise_on_status=True → MaxRetriesExceeded on exhaustion."""
    # /flaky/N endpoint fails twice then succeeds. With total=1, we exhaust
    # before success and the last response status is in forcelist (503).
    retries = rqx.Retry(
        total=1,
        backoff_factor=0.0,
        status_forcelist={503},
        raise_on_status=True,  # explicit
    )
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport)
    with pytest.raises(rqx.MaxRetriesExceeded):
        client.get(f"{flaky_server}/?request_id=raise_on_status_true_test")


def test_raise_on_status_false_returns_response(flaky_server):
    """raise_on_status=False → return the failing response, don't raise."""
    retries = rqx.Retry(
        total=1,
        backoff_factor=0.0,
        status_forcelist={503},
        raise_on_status=False,
    )
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport)
    # Should NOT raise, should return the 503
    resp = client.get(f"{flaky_server}/?request_id=raise_on_status_false_test")
    assert resp.status_code == 503


# ----- raise_on_redirect -----


def test_raise_on_redirect_true_raises_on_loop(flaky_server):
    """Default: raise_on_redirect=True → TooManyRedirects on loop."""
    retries = rqx.Retry(raise_on_redirect=True)
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, follow_redirects=True, max_redirects=2)
    with pytest.raises(rqx.TooManyRedirects):
        client.get(f"{flaky_server}/redirect-loop")


def test_raise_on_redirect_false_returns_3xx(flaky_server):
    """raise_on_redirect=False → return the last 3xx response."""
    retries = rqx.Retry(raise_on_redirect=False)
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, follow_redirects=True, max_redirects=2)
    resp = client.get(f"{flaky_server}/redirect-loop")
    assert 300 <= resp.status_code < 400


# ----- async variants -----


@pytest.mark.asyncio
async def test_raise_on_status_false_returns_response_async(flaky_server):
    retries = rqx.Retry(
        total=1,
        backoff_factor=0.0,
        status_forcelist={503},
        raise_on_status=False,
    )
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(f"{flaky_server}/?request_id=async_raise_on_status_false")
    assert resp.status_code == 503


@pytest.mark.asyncio
async def test_raise_on_redirect_false_returns_3xx_async(flaky_server):
    retries = rqx.Retry(raise_on_redirect=False)
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(transport=transport, follow_redirects=True, max_redirects=2)
    resp = await client.get(f"{flaky_server}/redirect-loop")
    assert 300 <= resp.status_code < 400
