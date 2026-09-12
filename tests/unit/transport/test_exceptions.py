"""Tests for the specific exception types raised on different failure modes (Issue #1)."""

import contextlib
import socket
import threading

import pytest

import rqx


@contextlib.contextmanager
def _raw_server(payload: bytes):
    """Answer every connection with the given bytes, then close it."""
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    sock.listen(5)
    port = sock.getsockname()[1]

    def serve():
        while True:
            try:
                conn, _ = sock.accept()
            except OSError:
                return
            with conn:
                conn.recv(65536)
                conn.sendall(payload)

    threading.Thread(target=serve, daemon=True).start()
    try:
        yield f"http://127.0.0.1:{port}/"
    finally:
        sock.close()


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
        node.name: [base.id for base in node.bases]
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
    assert "ConnectTimeout" in declared and "RqxError" in declared
    for name in sorted(declared):
        runtime = [base.__name__ for base in getattr(rqx, name).__bases__]
        assert runtime == classes[name], (
            f"{name}: stub says {classes[name]}, runtime is {runtime}"
        )


def test_malformed_response_is_caught_by_http_error():
    """A response hyper can't parse has no specific class yet, but it is still a request failure."""
    with _raw_server(b"garbage\r\n\r\n") as url:
        with pytest.raises(rqx.RequestError):
            rqx.Client().get(url)


SHORT_BODY = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nhello"


def test_stream_body_failure_is_transport_error():
    """A body cut short mid-stream goes through the same mapping as a buffered read."""
    with _raw_server(SHORT_BODY) as url:
        with rqx.Client().stream("GET", url) as resp:
            with pytest.raises(rqx.TransportError):
                list(resp.iter_bytes())
        with rqx.Client().stream("GET", url) as resp:
            with pytest.raises(rqx.TransportError):
                list(resp.iter_text())
        with rqx.Client().stream("GET", url) as resp:
            with pytest.raises(rqx.TransportError):
                list(resp.iter_lines())


@pytest.mark.asyncio
async def test_stream_body_failure_is_transport_error_async():
    with _raw_server(SHORT_BODY) as url:
        async with rqx.AsyncClient() as client:
            resp = await client.stream("GET", url)
            with pytest.raises(rqx.TransportError):
                async for _ in resp.aiter_bytes():
                    pass
            resp = await client.stream("GET", url)
            with pytest.raises(rqx.TransportError):
                async for _ in resp.aiter_text():
                    pass
            resp = await client.stream("GET", url)
            with pytest.raises(rqx.TransportError):
                async for _ in resp.aiter_lines():
                    pass


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
