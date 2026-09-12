"""Exception types per failure mode, and the shape of the hierarchy."""

import socket

import pytest


def _closed_port():
    """An ephemeral port that was just released, so a connect is refused.
    Holding the socket bound instead would be race-free on Linux, but macOS
    drops SYNs to a bound, non-listening port and the connect hangs."""
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


def test_http_status_error_is_not_a_request_error(lib):
    assert not issubclass(lib.module.HTTPStatusError, lib.module.RequestError)


def test_http_error_catches_status_and_transport_errors(lib, flaky_server):
    resp = lib.client().delete(f"{flaky_server}/no-such-route")
    with pytest.raises(lib.module.HTTPError):
        resp.raise_for_status()
    with pytest.raises(lib.module.HTTPError):
        lib.client().get(f"http://127.0.0.1:{_closed_port()}/")


# ----- issue #182: the server's bytes decide the class, same rules as httpx -----

GARBAGE_HEAD = b"garbage\r\n\r\n"
SHORT_BODY = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nhello"
CORRUPT_GZIP = (
    b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 5\r\n\r\nxxxxx"
)
CONNECT_REFUSED = b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n"


def test_garbage_status_line_is_remote_protocol_error(lib, canned_server):
    with pytest.raises(lib.module.RemoteProtocolError):
        lib.client().get(canned_server(GARBAGE_HEAD))


def test_server_hangup_before_response_is_remote_protocol_error(lib, canned_server):
    with pytest.raises(lib.module.RemoteProtocolError):
        lib.client().get(canned_server(b""))


def test_short_body_is_remote_protocol_error(lib, canned_server):
    with pytest.raises(lib.module.RemoteProtocolError):
        lib.client().get(canned_server(SHORT_BODY))


def test_corrupt_gzip_is_decoding_error(lib, canned_server):
    with pytest.raises(lib.module.DecodingError):
        lib.client().get(canned_server(CORRUPT_GZIP))


def test_reset_mid_body_is_read_error(lib, canned_server):
    with pytest.raises(lib.module.ReadError):
        lib.client().get(canned_server(SHORT_BODY, reset=True))


@pytest.mark.parametrize("url", ["example.com", "ftp://example.com/"])
def test_bad_scheme_is_unsupported_protocol(lib, url):
    with pytest.raises(lib.module.UnsupportedProtocol):
        lib.client().get(url)


def test_proxy_refusing_connect_is_proxy_error(lib, canned_server):
    with pytest.raises(lib.module.ProxyError):
        lib.proxied_client(canned_server(CONNECT_REFUSED)).get("https://example.com/")


def test_unreachable_proxy_is_connect_error(lib):
    with pytest.raises(lib.module.ConnectError):
        lib.proxied_client(f"http://127.0.0.1:{_closed_port()}").get(
            "https://example.com/"
        )


def test_proxy_reply_for_plain_http_is_the_response(lib, canned_server):
    resp = lib.proxied_client(canned_server(CONNECT_REFUSED)).get("http://example.com/")
    assert resp.status_code == 503
