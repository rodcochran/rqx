"""Tests for retry config fields that affect runtime behavior (Issue #2).

Covers:
- backoff_jitter — randomizes backoff timing
- raise_on_status — toggle raise vs return on retry exhaustion
- raise_on_redirect — toggle raise vs return on redirect-loop exhaustion
"""

import time

import pytest

import rqx
from conftest import FlakyServerHandler, _free_port


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


# ----- retries under follow_redirects -----
#
# Every send goes through Transport::send, so retries apply to redirect hops
# and streaming too (https://github.com/rodcochran/rqx/issues/148). Caps apply per hop; telemetry adds up across the
# chain. The control test is identical except for follow_redirects.


def test_retries_fire_without_redirect_control(flaky_server):
    """Control: retries work on the flaky endpoint when not following redirects."""
    retries = rqx.Retry(total=5, backoff_factor=0.0, status_forcelist={503})
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, follow_redirects=False)

    resp = client.get(f"{flaky_server}/?request_id=retry_no_redirect_control")
    assert resp.status_code == 200


def test_retries_fire_under_follow_redirects(flaky_server):
    """Retry config must still apply while following a redirect chain.

    /redirect-to-flaky 302s to the flaky endpoint, which returns 503 for its
    first two hits and 200 on the third. With total=5 the retry loop rides
    through to the 200, and the telemetry on the final response reflects the
    two retries spent on the second hop.
    """
    retries = rqx.Retry(total=5, backoff_factor=0.0, status_forcelist={503})
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, follow_redirects=True)

    resp = client.get(
        f"{flaky_server}/redirect-to-flaky?request_id=retry_under_redirect_sync"
    )
    assert resp.status_code == 200
    assert resp.num_retries == 2
    assert [status for status, _ in resp.retry_history] == ["503", "200"]


@pytest.mark.asyncio
async def test_retries_fire_under_follow_redirects_async(flaky_server):
    """Async path shares Client::request, so it takes the same route."""
    retries = rqx.Retry(total=5, backoff_factor=0.0, status_forcelist={503})
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(transport=transport, follow_redirects=True)

    resp = await client.get(
        f"{flaky_server}/redirect-to-flaky?request_id=retry_under_redirect_async"
    )
    assert resp.status_code == 200
    assert resp.num_retries == 2


def test_retries_fire_on_stream_without_redirect(flaky_server):
    """Streaming used to bypass the retry loop entirely, redirects or not."""
    retries = rqx.Retry(total=5, backoff_factor=0.0, status_forcelist={503})
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, follow_redirects=False)

    with client.stream(
        "GET", f"{flaky_server}/?request_id=retry_stream_no_redirect"
    ) as resp:
        assert resp.status_code == 200
        assert resp.num_retries == 2
        assert b"".join(resp.iter_bytes())


def test_retries_fire_on_stream_under_follow_redirects(flaky_server):
    retries = rqx.Retry(total=5, backoff_factor=0.0, status_forcelist={503})
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, follow_redirects=True)

    with client.stream(
        "GET", f"{flaky_server}/redirect-to-flaky?request_id=retry_stream_redirect_sync"
    ) as resp:
        assert resp.status_code == 200
        assert resp.num_retries == 2


@pytest.mark.asyncio
async def test_retries_fire_on_stream_under_follow_redirects_async(flaky_server):
    retries = rqx.Retry(total=5, backoff_factor=0.0, status_forcelist={503})
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(transport=transport, follow_redirects=True)

    resp = await client.stream(
        "GET",
        f"{flaky_server}/redirect-to-flaky?request_id=retry_stream_redirect_async",
    )
    async with resp:
        assert resp.status_code == 200
        assert resp.num_retries == 2


# ----- retry caps apply per hop -----
#
# /flaky-redirect: 503, 503, 302 -> destination: 503, 503, 200. Four retries
# over two hops. total=3 passes per hop and would fail if shared across the chain.


def test_retry_caps_apply_per_redirect_hop(flaky_server):
    retries = rqx.Retry(total=3, backoff_factor=0.0, status_forcelist={503})
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, follow_redirects=True)

    resp = client.get(f"{flaky_server}/flaky-redirect?request_id=caps_per_hop_sync")
    assert resp.status_code == 200
    assert resp.num_retries == 4
    assert [status for status, _ in resp.retry_history] == ["503", "302", "503", "200"]


@pytest.mark.asyncio
async def test_retry_caps_apply_per_redirect_hop_async(flaky_server):
    retries = rqx.Retry(total=3, backoff_factor=0.0, status_forcelist={503})
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(transport=transport, follow_redirects=True)

    resp = await client.get(
        f"{flaky_server}/flaky-redirect?request_id=caps_per_hop_async"
    )
    assert resp.status_code == 200
    assert resp.num_retries == 4
    assert [status for status, _ in resp.retry_history] == ["503", "302", "503", "200"]


# ----- per-kind caps (https://github.com/rodcochran/rqx/issues/54) -----
# flaky endpoint: 503 twice then 200; /reset: closes after accept; free port: refused.


def test_status_cap_stops_before_total(flaky_server):
    retries = rqx.Retry(
        total=5, status=1, backoff_factor=0.0, status_forcelist={503}, raise_on_status=False
    )
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    resp = client.get(f"{flaky_server}/?request_id=status_cap_sync")
    assert resp.status_code == 503
    assert resp.num_retries == 1


def test_total_still_caps_a_generous_status_cap(flaky_server):
    retries = rqx.Retry(
        total=1, status=5, backoff_factor=0.0, status_forcelist={503}, raise_on_status=False
    )
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    resp = client.get(f"{flaky_server}/?request_id=total_caps_status_sync")
    assert resp.status_code == 503
    assert resp.num_retries == 1


def test_status_cap_exhaustion_message_reports_breakdown(flaky_server):
    retries = rqx.Retry(total=5, status=1, backoff_factor=0.0, status_forcelist={503})
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    with pytest.raises(rqx.MaxRetriesExceeded, match=r"1 retries \(0 connect, 0 read, 1 status\)"):
        client.get(f"{flaky_server}/?request_id=status_cap_message")


def test_connect_cap_stops_before_total():
    retries = rqx.Retry(total=5, connect=1, backoff_factor=0.0)
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    with pytest.raises(rqx.MaxRetriesExceeded, match=r"\(1 connect, 0 read, 0 status\)"):
        client.get(f"http://127.0.0.1:{_free_port()}/")


def test_read_cap_stops_before_total(flaky_server):
    retries = rqx.Retry(total=5, read=1, backoff_factor=0.0)
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    with pytest.raises(rqx.MaxRetriesExceeded, match=r"\(0 connect, 1 read, 0 status\)"):
        client.get(f"{flaky_server}/reset?request_id=read_cap_sync")
    assert FlakyServerHandler.counters["read_cap_sync"] == 2  # 1 attempt + 1 retry


def test_caps_default_to_total(flaky_server):
    retries = rqx.Retry(total=3, backoff_factor=0.0, status_forcelist={503})
    assert (retries.connect, retries.read, retries.status) == (3, 3, 3)
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    resp = client.get(f"{flaky_server}/?request_id=caps_default_sync")
    assert resp.status_code == 200
    assert resp.num_retries == 2


@pytest.mark.asyncio
async def test_status_cap_stops_before_total_async(flaky_server):
    retries = rqx.Retry(
        total=5, status=1, backoff_factor=0.0, status_forcelist={503}, raise_on_status=False
    )
    client = rqx.AsyncClient(transport=rqx.AsyncHTTPTransport(retries=retries))
    resp = await client.get(f"{flaky_server}/?request_id=status_cap_async")
    assert resp.status_code == 503
    assert resp.num_retries == 1


def test_zero_cap_means_no_retry_of_that_kind(flaky_server):
    retries = rqx.Retry(
        total=5, status=0, backoff_factor=0.0, status_forcelist={503}, raise_on_status=False
    )
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    resp = client.get(f"{flaky_server}/?request_id=zero_status_cap")
    assert resp.status_code == 503
    assert resp.num_retries == 0


def test_mixed_kinds_are_charged_separately(flaky_server):
    """/reset-then-flaky: one reset (read), two 503s (status), then 200."""
    retries = rqx.Retry(
        total=5, read=1, status=2, backoff_factor=0.0, status_forcelist={503}
    )
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    resp = client.get(f"{flaky_server}/reset-then-flaky?request_id=mixed_kinds")
    assert resp.status_code == 200
    assert resp.num_retries == 3
    # the reset is attempt 0, which retry_history never records
    assert [status for status, _ in resp.retry_history] == ["503", "503", "200"]


def test_mixed_kinds_stop_at_the_first_exhausted_cap(flaky_server):
    retries = rqx.Retry(
        total=5, read=0, status=2, backoff_factor=0.0, status_forcelist={503}
    )
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retries))
    with pytest.raises(rqx.MaxRetriesExceeded, match=r"0 retries \(0 connect, 0 read, 0 status\)"):
        client.get(f"{flaky_server}/reset-then-flaky?request_id=mixed_kinds_read_zero")
    assert FlakyServerHandler.counters["mixed_kinds_read_zero"] == 1
