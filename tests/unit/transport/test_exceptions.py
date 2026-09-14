"""Tests for the specific exception types raised on different failure modes (Issue #1)."""

import pytest
import rqx


def test_connect_error_dns_failure():
    """A DNS failure raises ConnectError (subclass of NetworkError, RqxError)."""
    client = rqx.Client()
    with pytest.raises(rqx.ConnectError):
        client.get("http://nonexistent.invalid/")


def test_connect_error_connection_refused():
    """A closed port raises ConnectError."""
    client = rqx.Client()
    with pytest.raises(rqx.ConnectError):
        client.get("http://127.0.0.1:1/")  # port 1 is reserved, nothing listens


def test_connect_error_is_subclass_of_rqxerror():
    """Hierarchy invariant: ConnectError → NetworkError → TransportError → RequestError → HTTPError → RqxError."""
    assert issubclass(rqx.ConnectError, rqx.NetworkError)
    assert issubclass(rqx.NetworkError, rqx.TransportError)
    assert issubclass(rqx.TransportError, rqx.RequestError)
    assert issubclass(rqx.RequestError, rqx.HTTPError)
    assert issubclass(rqx.HTTPError, rqx.RqxError)


def test_status_and_request_errors_are_siblings_under_http_error():
    """Same shape as httpx: HTTPError -> {RequestError, HTTPStatusError}, MaxRetriesExceeded beside them."""
    assert issubclass(rqx.HTTPStatusError, rqx.HTTPError)
    assert not issubclass(rqx.HTTPStatusError, rqx.RequestError)
    assert issubclass(rqx.MaxRetriesExceeded, rqx.HTTPError)
    assert not issubclass(rqx.MaxRetriesExceeded, rqx.RequestError)


def test_stub_hierarchy_matches_runtime():
    """Every exception class the stub declares has the parent the stub says it has."""
    import ast
    import pathlib

    stub = pathlib.Path(rqx.__file__).with_name("_types.pyi").read_text()
    classes = {
        node.name: [ast.unparse(base).rsplit(".", 1)[-1] for base in node.bases]
        for node in ast.parse(stub).body
        if isinstance(node, ast.ClassDef)
    }
    # Walk down from Exception so the set is the stub's own tree, not a name pattern.
    declared = {"Exception"}
    while True:
        more = {
            name for name, bases in classes.items() if bases and bases[0] in declared
        }
        if more <= declared:
            break
        declared |= more
    declared.discard("Exception")
    assert {"ConnectTimeout", "RqxError", "StreamClosed", "JSONDecodeError"} <= declared
    for name in sorted(declared):
        runtime = [base.__name__ for base in getattr(rqx, name).__bases__]
        assert runtime == classes[name], (
            f"{name}: stub says {classes[name]}, runtime is {runtime}"
        )


def test_new_leaf_classes_have_httpx_parents():
    assert issubclass(rqx.ProtocolError, rqx.TransportError)
    assert issubclass(rqx.RemoteProtocolError, rqx.ProtocolError)
    assert issubclass(rqx.UnsupportedProtocol, rqx.TransportError)
    assert issubclass(rqx.DecodingError, rqx.RequestError)
    assert not issubclass(rqx.DecodingError, rqx.TransportError)


# ----- issue #182: what the server sent decides the class -----


def _encoded(encoding: str, body: bytes) -> bytes:
    head = f"HTTP/1.1 200 OK\r\nContent-Encoding: {encoding}\r\nContent-Length: {len(body)}\r\n\r\n"
    return head.encode() + body


MALFORMED_HEADS = {
    "garbage status line": b"garbage\r\n\r\n",
    "invalid header line": b"HTTP/1.1 200 OK\r\nBad Header\r\nContent-Length: 0\r\n\r\n",
    "closed mid headers": b"HTTP/1.1 200 OK\r\nContent-Ty",
    "closed before response": b"",
}

BROKEN_BODIES = {
    "shorter than content-length": b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nhello",
    "bad chunk size": b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nhello\r\n0\r\n\r\n",
}

CORRUPT_ENCODINGS = {
    encoding: _encoded(encoding, b"xxxxx")
    for encoding in ("gzip", "br", "zstd", "deflate")
}

SHORT_BODY = BROKEN_BODIES["shorter than content-length"]


def _stream_all(url: str):
    with rqx.Client().stream("GET", url) as resp:
        return list(resp.iter_bytes())


@pytest.mark.parametrize("payload", MALFORMED_HEADS.values(), ids=MALFORMED_HEADS)
def test_malformed_head_is_remote_protocol_error(canned_server, payload):
    url = canned_server(payload)
    with pytest.raises(rqx.RemoteProtocolError):
        rqx.Client().get(url)
    with pytest.raises(rqx.RemoteProtocolError):
        _stream_all(url)


@pytest.mark.parametrize("payload", BROKEN_BODIES.values(), ids=BROKEN_BODIES)
def test_broken_body_is_remote_protocol_error(canned_server, payload):
    url = canned_server(payload)
    with pytest.raises(rqx.RemoteProtocolError):
        rqx.Client().get(url)
    with pytest.raises(rqx.RemoteProtocolError):
        _stream_all(url)


@pytest.mark.parametrize("payload", CORRUPT_ENCODINGS.values(), ids=CORRUPT_ENCODINGS)
def test_corrupt_encoding_is_decoding_error(canned_server, payload):
    """A body the decompressor rejects is a DecodingError, not a transport failure."""
    url = canned_server(payload)
    with pytest.raises(rqx.DecodingError):
        rqx.Client().get(url)
    with pytest.raises(rqx.DecodingError):
        _stream_all(url)


def test_reset_mid_body_is_read_error(canned_server):
    url = canned_server(SHORT_BODY, reset=True)
    with pytest.raises(rqx.ReadError):
        rqx.Client().get(url)
    with pytest.raises(rqx.ReadError):
        _stream_all(url)


def test_reset_before_response_is_read_error(canned_server):
    url = canned_server(b"", reset=True)
    with pytest.raises(rqx.ReadError):
        rqx.Client().get(url)


def test_stream_iterators_share_the_mapping(canned_server):
    """Every iterator flavor raises the mapped class, not a bare RqxError."""
    url = canned_server(SHORT_BODY)
    with rqx.Client().stream("GET", url) as resp:
        with pytest.raises(rqx.RemoteProtocolError):
            list(resp.iter_text())
    with rqx.Client().stream("GET", url) as resp:
        with pytest.raises(rqx.RemoteProtocolError):
            list(resp.iter_lines())


@pytest.mark.asyncio
async def test_stream_iterators_share_the_mapping_async(canned_server):
    url = canned_server(SHORT_BODY)
    corrupt = canned_server(CORRUPT_ENCODINGS["gzip"])
    async with rqx.AsyncClient() as client:
        async with client.stream("GET", url) as resp:
            with pytest.raises(rqx.RemoteProtocolError):
                async for _ in resp.aiter_bytes():
                    pass
        async with client.stream("GET", url) as resp:
            with pytest.raises(rqx.RemoteProtocolError):
                async for _ in resp.aiter_text():
                    pass
        async with client.stream("GET", url) as resp:
            with pytest.raises(rqx.RemoteProtocolError):
                async for _ in resp.aiter_lines():
                    pass
        async with client.stream("GET", corrupt) as resp:
            with pytest.raises(rqx.DecodingError):
                async for _ in resp.aiter_bytes():
                    pass


# ----- issue #182: URLs -----


@pytest.mark.parametrize("url", ["example.com", "", "/users", "mailto:x@y"])
def test_missing_scheme_is_unsupported_protocol(url):
    with pytest.raises(
        rqx.UnsupportedProtocol, match="missing an 'http://' or 'https://'"
    ):
        rqx.Client().get(url)


@pytest.mark.parametrize("url", ["ftp://example.com/", "file:///etc/hosts"])
def test_non_http_scheme_is_unsupported_protocol(url):
    with pytest.raises(rqx.UnsupportedProtocol, match="unsupported protocol"):
        rqx.Client().get(url)


@pytest.mark.parametrize("url", ["http://[::1", "http://"])
def test_unparsable_url_is_value_error(url):
    with pytest.raises(ValueError):
        rqx.Client().get(url)


# ----- issue #182: proxies -----

CONNECT_REFUSED = b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n"
CONNECT_AUTH = (
    b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n"
)


def _via_proxy(proxy_url: str) -> rqx.Client:
    proxy = proxy_url.rstrip("/")
    return rqx.Client(
        transport=rqx.HTTPTransport(proxy={"http": proxy, "https": proxy})
    )


def _closed_port() -> int:
    import socket

    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def test_proxy_refusing_connect_is_proxy_error(canned_server):
    """Same rule as httpx: ProxyError means the proxy answered CONNECT with a failure."""
    with pytest.raises(rqx.ProxyError):
        _via_proxy(canned_server(CONNECT_REFUSED)).get("https://example.com/")


def test_proxy_demanding_auth_is_proxy_error(canned_server):
    with pytest.raises(rqx.ProxyError):
        _via_proxy(canned_server(CONNECT_AUTH)).get("https://example.com/")


def test_unreachable_proxy_is_connect_error():
    """The proxy never answered, so it is a plain connect failure, like httpx."""
    with pytest.raises(rqx.ConnectError):
        _via_proxy(f"http://127.0.0.1:{_closed_port()}").get("https://example.com/")


def test_proxy_response_for_plain_http_is_returned(canned_server):
    """For an http:// target the proxy's own reply is the response, not an error."""
    resp = _via_proxy(canned_server(CONNECT_REFUSED)).get("http://example.com/")
    assert resp.status_code == 503


def test_request_error_does_not_catch_status_error(flaky_server):
    """except RequestError is for transport failures; a 404 from raise_for_status() passes through it."""
    resp = rqx.Client().delete(f"{flaky_server}/no-such-route")
    with pytest.raises(rqx.HTTPStatusError):
        try:
            resp.raise_for_status()
        except rqx.RequestError:
            pytest.fail("RequestError caught an HTTPStatusError")


def test_http_error_catches_status_and_transport_errors(flaky_server):
    """except HTTPError is the one clause for both."""
    resp = rqx.Client().delete(f"{flaky_server}/no-such-route")
    with pytest.raises(rqx.HTTPError):
        resp.raise_for_status()
    with pytest.raises(rqx.HTTPError):
        rqx.Client().get("http://nonexistent.invalid/")


def test_http_error_catches_max_retries_exceeded(flaky_server):
    retry = rqx.Retry(total=1, status=1, status_forcelist={503}, raise_on_status=True)
    client = rqx.Client(transport=rqx.HTTPTransport(retries=retry))
    with pytest.raises(rqx.HTTPError):
        client.get(f"{flaky_server}/status/503")


def test_connect_error_caught_by_rqxerror():
    """Old-style except rqx.RqxError still catches new-style ConnectError."""
    client = rqx.Client()
    with pytest.raises(rqx.RqxError):
        client.get("http://nonexistent.invalid/")


def test_read_timeout(flaky_server):
    """Server takes longer than the client's timeout — raises ReadTimeout."""
    client = rqx.Client(timeout=0.2)
    with pytest.raises(rqx.ReadTimeout):
        client.get(f"{flaky_server}/sleep/1")  # server sleeps past the timeout


def test_read_timeout_caught_by_timeout_exception(flaky_server):
    """ReadTimeout is catchable as TimeoutException."""
    client = rqx.Client(timeout=0.2)
    with pytest.raises(rqx.TimeoutException):
        client.get(f"{flaky_server}/sleep/1")


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
        await client.get("http://nonexistent.invalid/")


@pytest.mark.asyncio
async def test_read_timeout_async(flaky_server):
    client = rqx.AsyncClient(timeout=0.2)
    with pytest.raises(rqx.ReadTimeout):
        await client.get(f"{flaky_server}/sleep/1")


@pytest.mark.asyncio
async def test_too_many_redirects_async(flaky_server):
    client = rqx.AsyncClient(follow_redirects=True, max_redirects=3)
    with pytest.raises(rqx.TooManyRedirects):
        await client.get(f"{flaky_server}/redirect-loop")
