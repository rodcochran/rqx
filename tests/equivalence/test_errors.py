"""Exception types per failure mode, and the shape of the hierarchy."""

import socket

import httpx
import pytest

ISSUE_114 = "https://github.com/rodcochran/rqx/issues/114"
ISSUE_88 = "https://github.com/rodcochran/rqx/issues/88"


def _closed_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def test_dns_failure_is_connect_error(lib):
    with pytest.raises(lib.module.ConnectError):
        lib.client().get("http://nonexistent.invalid/")


def test_connection_refused_is_connect_error(lib):
    with pytest.raises(lib.module.ConnectError):
        lib.client().get(f"http://127.0.0.1:{_closed_port()}/")


def test_slow_server_is_read_timeout(lib, flaky_server):
    with pytest.raises(lib.module.ReadTimeout):
        lib.client(timeout=0.2).get(f"{flaky_server}/sleep/1")


def test_untrusted_certificate_is_connect_error(lib, mtls_server):
    with pytest.raises(lib.module.ConnectError):
        lib.client().get(mtls_server)


def test_raise_for_status_raises_http_status_error(lib, flaky_server):
    resp = lib.client().delete(f"{flaky_server}/no-such-route")
    with pytest.raises(lib.module.HTTPStatusError):
        resp.raise_for_status()


@pytest.mark.rqx_diverges(
    issue=ISSUE_114,
    reason="rqx nests HTTPStatusError under RequestError; httpx does not",
)
def test_http_status_error_is_not_a_request_error(lib):
    assert not issubclass(lib.module.HTTPStatusError, lib.module.RequestError)


@pytest.mark.rqx_diverges(
    issue=ISSUE_88, reason="rqx exceptions are not subclasses of httpx's"
)
def test_connect_error_is_an_httpx_connect_error(lib):
    """Retry decorators registered against httpx's classes must keep firing."""
    with pytest.raises(httpx.ConnectError):
        lib.client().get(f"http://127.0.0.1:{_closed_port()}/")
