"""Type stubs for the compiled `rqx._rqx` extension module.

Mirrors the surface area exposed from src/lib.rs. Keep this in sync with the
Rust signatures — pyo3 doesn't generate stubs automatically.
"""

from collections.abc import AsyncIterator, Awaitable, Iterator, Mapping
from types import TracebackType
from typing import Any

from typing_extensions import TypeAlias

# ---------------------------------------------------------------------------
# Type aliases used across the API
# ---------------------------------------------------------------------------

# `verify=` accepts True/False or a path to a CA bundle.
VerifyTypes: TypeAlias = bool | str

# `cert=` accepts either a combined PEM (path or bytes) or a (cert, key) tuple.
CertTypes: TypeAlias = str | bytes | tuple[str, str]

# `timeout=` accepts a bare number (applies to all phases) or a Timeout instance.
TimeoutTypes = ...

# Proxy mapping: scheme ("http"/"https") -> proxy URL.
ProxyTypes: TypeAlias = Mapping[str, str]

# `params=` values are coerced like httpx: bool -> "true"/"false", None drops
# the key, int/float -> str(). See https://github.com/rodcochran/rqx/issues/115.
QueryParamValue: TypeAlias = str | int | float | bool | None
QueryParamTypes: TypeAlias = Mapping[str, QueryParamValue]

# `headers=` accepts a str -> str mapping or a Headers instance. Invalid names
# or values raise ValueError. See https://github.com/rodcochran/rqx/issues/117.
HeaderTypes: TypeAlias = Mapping[str, str] | PyHeaders

# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------
#
# Exception hierarchy (same shape as httpx):
#
#   RqxError
#     └── HTTPError
#           ├── RequestError
#           │     ├── TransportError
#           │     │     ├── TimeoutException
#           │     │     │     ├── ConnectTimeout
#           │     │     │     ├── ReadTimeout
#           │     │     │     ├── WriteTimeout
#           │     │     │     └── PoolTimeout
#           │     │     ├── NetworkError
#           │     │     │     ├── ConnectError
#           │     │     │     ├── ReadError
#           │     │     │     └── WriteError
#           │     │     ├── ProtocolError
#           │     │     │     └── RemoteProtocolError
#           │     │     ├── ProxyError
#           │     │     └── UnsupportedProtocol
#           │     ├── DecodingError
#           │     └── TooManyRedirects
#           ├── HTTPStatusError
#           └── MaxRetriesExceeded
#

class RqxError(Exception): ...
class HTTPError(RqxError): ...
class RequestError(HTTPError): ...
class HTTPStatusError(HTTPError): ...
class MaxRetriesExceeded(HTTPError): ...
class TransportError(RequestError): ...
class DecodingError(RequestError): ...
class TooManyRedirects(RequestError): ...
class TimeoutException(TransportError): ...
class NetworkError(TransportError): ...
class ProtocolError(TransportError): ...
class ProxyError(TransportError): ...
class UnsupportedProtocol(TransportError): ...
class RemoteProtocolError(ProtocolError): ...
class ConnectTimeout(TimeoutException): ...
class ReadTimeout(TimeoutException): ...
class WriteTimeout(TimeoutException): ...
class PoolTimeout(TimeoutException): ...
class ConnectError(NetworkError): ...
class ReadError(NetworkError): ...
class WriteError(NetworkError): ...

# ---------------------------------------------------------------------------
# Headers
# ---------------------------------------------------------------------------

class PyHeaders:
    """Case-insensitive header map, backed by http::HeaderMap on the Rust side."""

    def __init__(self, init: Mapping[str, str] | None = None) -> None: ...
    def __getitem__(self, key: str) -> str: ...
    def __setitem__(self, key: str, value: str) -> None: ...
    def __delitem__(self, key: str) -> None: ...
    def __contains__(self, key: str) -> bool: ...
    def __iter__(self) -> Iterator[str]: ...
    def __len__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def get(self, key: str, default: str | None = None) -> str | None: ...
    def keys(self) -> list[str]: ...
    def values(self) -> list[str]: ...
    def items(self) -> list[tuple[str, str]]: ...

# ---------------------------------------------------------------------------
# Timeout
# ---------------------------------------------------------------------------

class Timeout:
    """Granular per-phase HTTP timeouts.

    Pass a single value to apply to every phase, or use kwargs to set phases
    individually. Per-phase kwargs override the positional `all`.

    Phases:
      - connect: TCP/TLS connection establishment
      - read:    receiving response data
      - write:   sending request body (currently a no-op; reqwest doesn't
                 expose a per-phase write timeout)
      - pool:    connection pool idle timeout
    """

    connect: float | None
    read: float | None
    write: float | None
    pool: float | None

    def __init__(
        self,
        all: float | None = None,
        *,
        connect: float | None = None,
        read: float | None = None,
        write: float | None = None,
        pool: float | None = None,
    ) -> None: ...

# ---------------------------------------------------------------------------
# Retry config
# ---------------------------------------------------------------------------

class PyRetry:
    """total counts retries, not attempts: total=3 allows four attempts. Under
    follow_redirects the caps apply per hop; num_retries and retry_history on
    the final response add up across the chain."""

    def __init__(
        self,
        total: int | None = None,
        connect: int | None = None,
        read: int | None = None,
        status: int | None = None,
        backoff_factor: float | None = None,
        backoff_max: float | None = None,
        backoff_jitter: float | None = None,
        status_forcelist: set[int] | None = None,
        allowed_methods: set[str] | None = None,
        respect_retry_after_header: bool | None = None,
        raise_on_status: bool | None = None,
        raise_on_redirect: bool | None = None,
        total_timeout: float | None = None,
    ) -> None: ...

# ---------------------------------------------------------------------------
# Responses
# ---------------------------------------------------------------------------

class PyResponse:
    status_code: int
    headers: PyHeaders
    content: bytes
    url: str
    elapsed: float
    num_retries: int
    retry_history: list[tuple[str, float]]
    http_version: str
    cookies: dict[str, str]

    encoding: str

    @property
    def text(self) -> str: ...
    @property
    def is_informational(self) -> bool: ...
    @property
    def is_success(self) -> bool: ...
    @property
    def is_redirect(self) -> bool: ...
    @property
    def is_client_error(self) -> bool: ...
    @property
    def is_server_error(self) -> bool: ...
    @property
    def is_error(self) -> bool: ...
    def json(self) -> Any: ...
    def raise_for_status(self) -> None: ...

class PyStreamResponse:
    status_code: int
    headers: PyHeaders
    url: str
    elapsed: float
    num_retries: int
    retry_history: list[tuple[str, float]]
    http_version: str
    cookies: dict[str, str]

    encoding: str

    @property
    def content(self) -> bytes: ...
    @property
    def text(self) -> str: ...
    @property
    def is_informational(self) -> bool: ...
    @property
    def is_success(self) -> bool: ...
    @property
    def is_redirect(self) -> bool: ...
    @property
    def is_client_error(self) -> bool: ...
    @property
    def is_server_error(self) -> bool: ...
    @property
    def is_error(self) -> bool: ...
    @property
    def is_closed(self) -> bool: ...
    @property
    def is_consumed(self) -> bool: ...
    def iter_bytes(self, chunk_size: int | None = None) -> Iterator[bytes]: ...
    def iter_text(self, chunk_size: int | None = None) -> Iterator[str]: ...
    def iter_lines(self) -> Iterator[str]: ...
    def read(self) -> bytes: ...
    def close(self) -> None: ...
    def json(self) -> Any: ...

class PyAsyncStreamResponse:
    status_code: int
    headers: PyHeaders
    url: str
    elapsed: float
    num_retries: int
    retry_history: list[tuple[str, float]]
    http_version: str
    cookies: dict[str, str]

    encoding: str

    @property
    def content(self) -> bytes: ...
    @property
    def text(self) -> str: ...
    @property
    def is_informational(self) -> bool: ...
    @property
    def is_success(self) -> bool: ...
    @property
    def is_redirect(self) -> bool: ...
    @property
    def is_client_error(self) -> bool: ...
    @property
    def is_server_error(self) -> bool: ...
    @property
    def is_error(self) -> bool: ...
    @property
    def is_closed(self) -> bool: ...
    @property
    def is_consumed(self) -> bool: ...
    def aiter_bytes(self, chunk_size: int | None = None) -> AsyncIterator[bytes]: ...
    def aiter_text(self, chunk_size: int | None = None) -> AsyncIterator[str]: ...
    def aiter_lines(self) -> AsyncIterator[str]: ...
    async def aread(self) -> bytes: ...
    async def aclose(self) -> None: ...
    def json(self) -> Any: ...

class PyStreamContext:
    """Returned by Client.stream(); `with` sends the request and closes the response after."""

    def __enter__(self) -> PyStreamResponse: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...

class PyAsyncStreamContext:
    """Returned by AsyncClient.stream(); `async with` sends the request and closes the response after."""

    async def __aenter__(self) -> PyAsyncStreamResponse: ...
    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...

# ---------------------------------------------------------------------------
# Transports
# ---------------------------------------------------------------------------

class HTTPTransport:
    retries: PyRetry | None

    def __init__(
        self,
        retries: PyRetry | None = None,
        max_connections: int | None = None,
        max_keepalive_connections: int | None = None,
        keepalive_expiry: float | None = None,
        http1: bool | None = None,
        http2: bool | None = None,
        verify: VerifyTypes | None = None,
        cert: CertTypes | None = None,
        proxy: ProxyTypes | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> None: ...

class AsyncHTTPTransport:
    retries: PyRetry | None

    def __init__(
        self,
        retries: PyRetry | None = None,
        max_connections: int | None = None,
        max_keepalive_connections: int | None = None,
        keepalive_expiry: float | None = None,
        http1: bool | None = None,
        http2: bool | None = None,
        verify: VerifyTypes | None = None,
        cert: CertTypes | None = None,
        proxy: ProxyTypes | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> None: ...

# ---------------------------------------------------------------------------
# Sync client
# ---------------------------------------------------------------------------

class PyClient:
    cookies: dict[str, str]

    @property
    def base_url(self) -> str | None: ...
    def __init__(
        self,
        verify: VerifyTypes | None = None,
        cert: CertTypes | None = None,
        timeout: TimeoutTypes | None = None,
        follow_redirects: bool | None = None,
        max_redirects: int | None = None,
        base_url: str | None = None,
        auth_bearer: str | None = None,
        transport: HTTPTransport | None = None,
    ) -> None: ...
    def request(
        self,
        method: str,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyResponse: ...
    def get(
        self,
        url: str,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyResponse: ...
    def options(
        self,
        url: str,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyResponse: ...
    def head(
        self,
        url: str,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyResponse: ...
    def post(
        self,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyResponse: ...
    def put(
        self,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyResponse: ...
    def patch(
        self,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyResponse: ...
    def delete(
        self,
        url: str,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyResponse: ...
    def stream(
        self,
        method: str,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyStreamContext: ...
    def __enter__(self) -> PyClient: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None: ...

# ---------------------------------------------------------------------------
# Async client
# ---------------------------------------------------------------------------

class PyAsyncClient:
    cookies: dict[str, str]

    @property
    def base_url(self) -> str | None: ...
    def __init__(
        self,
        verify: VerifyTypes | None = None,
        cert: CertTypes | None = None,
        timeout: TimeoutTypes | None = None,
        follow_redirects: bool | None = None,
        max_redirects: int | None = None,
        base_url: str | None = None,
        auth_bearer: str | None = None,
        transport: AsyncHTTPTransport | None = None,
    ) -> None: ...
    def request(
        self,
        method: str,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> Awaitable[PyResponse]: ...
    def get(
        self,
        url: str,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> Awaitable[PyResponse]: ...
    def options(
        self,
        url: str,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> Awaitable[PyResponse]: ...
    def head(
        self,
        url: str,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> Awaitable[PyResponse]: ...
    def post(
        self,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> Awaitable[PyResponse]: ...
    def put(
        self,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> Awaitable[PyResponse]: ...
    def patch(
        self,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> Awaitable[PyResponse]: ...
    def delete(
        self,
        url: str,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> Awaitable[PyResponse]: ...
    def stream(
        self,
        method: str,
        url: str,
        content: bytes | None = None,
        data: Mapping[str, str] | None = None,
        json: Any | None = None,
        params: QueryParamTypes | None = None,
        headers: HeaderTypes | None = None,
        auth: tuple[str, str] | None = None,
        auth_bearer: str | None = None,
        follow_redirects: bool | None = None,
        timeout: TimeoutTypes | None = None,
    ) -> PyAsyncStreamContext: ...
    async def __aenter__(self) -> PyAsyncClient: ...
    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> bool: ...
