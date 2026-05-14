"""Tests for the Timeout class and granular per-phase timeouts (Issue #16).

Covers:
  - bare-number `timeout=N` shorthand (sets all four phases)
  - `Timeout(all=N)` positional shorthand
  - per-phase kwargs (connect / read / write / pool)
  - per-phase kwargs override the positional `all`
  - read timeout bites against a slow endpoint
  - connect timeout bites against an unroutable host
  - per-request `timeout=` overrides the client default
  - constructor argument validation
  - mutual exclusion with transport=
"""

import asyncio
import time

import pytest

import rqx


# ---------------------------------------------------------------------------
# Constructor / API surface
# ---------------------------------------------------------------------------


def test_timeout_bare_number_fills_all_phases():
    t = rqx.Timeout(5.0)
    assert t.connect == 5.0
    assert t.read == 5.0
    assert t.write == 5.0
    assert t.pool == 5.0


def test_timeout_per_phase_kwargs():
    t = rqx.Timeout(connect=1.0, read=2.0, write=3.0, pool=4.0)
    assert t.connect == 1.0
    assert t.read == 2.0
    assert t.write == 3.0
    assert t.pool == 4.0


def test_timeout_kwargs_override_all():
    """When both `all` and a per-phase kwarg are passed, per-phase wins."""
    t = rqx.Timeout(5.0, read=10.0)
    assert t.connect == 5.0
    assert t.read == 10.0
    assert t.write == 5.0
    assert t.pool == 5.0


def test_timeout_partial_kwargs_leaves_others_none():
    """Per-phase kwarg without `all` only sets that one phase."""
    t = rqx.Timeout(connect=1.0)
    assert t.connect == 1.0
    assert t.read is None
    assert t.write is None
    assert t.pool is None


def test_timeout_no_args_all_none():
    t = rqx.Timeout()
    assert t.connect is None
    assert t.read is None
    assert t.write is None
    assert t.pool is None


def test_timeout_repr():
    t = rqx.Timeout(connect=1.5)
    r = repr(t)
    assert "Timeout(" in r
    assert "connect=Some(1.5)" in r
    assert "read=None" in r


# ---------------------------------------------------------------------------
# Client constructor accepts Timeout
# ---------------------------------------------------------------------------


def test_client_accepts_bare_number():
    rqx.Client(timeout=1.0)  # shouldn't raise


def test_client_accepts_timeout_instance():
    rqx.Client(timeout=rqx.Timeout(connect=1.0, read=2.0))


def test_client_rejects_invalid_type():
    with pytest.raises(TypeError):
        rqx.Client(timeout="ten seconds")


def test_async_client_accepts_timeout_instance():
    rqx.AsyncClient(timeout=rqx.Timeout(connect=1.0, read=2.0))


def test_client_transport_and_timeout_conflict():
    """transport= and timeout= together raise; pick one path."""
    t = rqx.HTTPTransport()
    with pytest.raises(rqx.RqxError):
        rqx.Client(transport=t, timeout=rqx.Timeout(read=1.0))


# ---------------------------------------------------------------------------
# Read timeout actually bites
# ---------------------------------------------------------------------------


def test_read_timeout_via_bare_number(flaky_server):
    client = rqx.Client(timeout=1.0)
    with pytest.raises(rqx.ReadTimeout):
        client.get(f"{flaky_server}/sleep/3")


def test_read_timeout_via_timeout_instance(flaky_server):
    client = rqx.Client(timeout=rqx.Timeout(read=1.0))
    with pytest.raises(rqx.ReadTimeout):
        client.get(f"{flaky_server}/sleep/3")


def test_read_timeout_does_not_fire_when_under_limit(flaky_server):
    """A response that finishes well under the read timeout succeeds."""
    client = rqx.Client(timeout=rqx.Timeout(read=5.0))
    resp = client.get(f"{flaky_server}/sleep/0.1")
    assert resp.status_code == 200


# ---------------------------------------------------------------------------
# Connect timeout actually bites
# ---------------------------------------------------------------------------


# 10.255.255.1 is a non-routable address on most networks; SYNs are dropped
# rather than refused, so a connect attempt hangs until the timeout fires.
UNROUTABLE_HOST = "http://10.255.255.1:8080/"


def test_connect_timeout_bites():
    client = rqx.Client(timeout=rqx.Timeout(connect=0.5))
    start = time.monotonic()
    with pytest.raises(rqx.RqxError) as excinfo:
        client.get(UNROUTABLE_HOST)
    elapsed = time.monotonic() - start
    # Should fire well under the read-fallback default (15s) and around the
    # configured connect_timeout. Generous upper bound to keep CI happy.
    assert elapsed < 5.0
    # ConnectTimeout is the precise type; accept TimeoutException as a fallback
    # in case the platform classifies the error as a generic timeout.
    assert isinstance(excinfo.value, (rqx.ConnectTimeout, rqx.TimeoutException))


# ---------------------------------------------------------------------------
# Per-request override
# ---------------------------------------------------------------------------


def test_per_request_timeout_overrides_client_default(flaky_server):
    """Client says 10s, per-request says 0.5s — the 0.5s should bite."""
    client = rqx.Client(timeout=10.0)
    with pytest.raises(rqx.ReadTimeout):
        client.get(f"{flaky_server}/sleep/2", timeout=0.5)


def test_per_request_timeout_with_instance(flaky_server):
    client = rqx.Client(timeout=10.0)
    with pytest.raises(rqx.ReadTimeout):
        client.get(f"{flaky_server}/sleep/2", timeout=rqx.Timeout(read=0.5))


def test_per_request_cannot_loosen_transport_read_timeout(flaky_server):
    """Client-level timeout installs a transport-wide read_timeout on the
    reqwest Client. Per-request `timeout=` is only a request-level total — it
    can tighten the budget but cannot relax the transport's per-phase ceiling.
    Document this as the contract so it doesn't surprise callers."""
    client = rqx.Client(timeout=0.5)
    with pytest.raises(rqx.ReadTimeout):
        client.get(f"{flaky_server}/sleep/1", timeout=5.0)


# ---------------------------------------------------------------------------
# Async parity
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_async_read_timeout(flaky_server):
    client = rqx.AsyncClient(timeout=rqx.Timeout(read=1.0))
    with pytest.raises(rqx.ReadTimeout):
        await client.get(f"{flaky_server}/sleep/3")


@pytest.mark.asyncio
async def test_async_per_request_timeout_override(flaky_server):
    client = rqx.AsyncClient(timeout=10.0)
    with pytest.raises(rqx.ReadTimeout):
        await client.get(f"{flaky_server}/sleep/2", timeout=0.5)


# ---------------------------------------------------------------------------
# Transport-level timeout
# ---------------------------------------------------------------------------


def test_transport_accepts_timeout(flaky_server):
    """Timeout passed to HTTPTransport applies to clients using that transport."""
    transport = rqx.HTTPTransport(timeout=rqx.Timeout(read=1.0))
    client = rqx.Client(transport=transport)
    with pytest.raises(rqx.ReadTimeout):
        client.get(f"{flaky_server}/sleep/3")
