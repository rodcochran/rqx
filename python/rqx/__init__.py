# import from the compiled Rust extension module
from ._rqx import (
    AsyncHTTPTransport,
    ConnectError,
    ConnectTimeout,
    HTTPStatusError,
    HTTPTransport,
    MaxRetriesExceeded,
    NetworkError,
    PoolTimeout,
    ProxyError,
    PyAsyncClient,
    PyClient,
    PyHeaders,
    PyRetry,
    ReadError,
    ReadTimeout,
    RequestError,
    RqxError,
    Timeout,
    TimeoutException,
    TooManyRedirects,
    TransportError,
    WriteError,
    WriteTimeout,
)

# module-level convenience functions — mirrors httpx.{get,post,...,stream}
from ._api import (
    delete,
    get,
    head,
    options,
    patch,
    post,
    put,
    request,
    stream,
)

# optional: nicer names (drop Py prefix)
Client = PyClient
AsyncClient = PyAsyncClient
Headers = PyHeaders
Retry = PyRetry

__all__ = [
    "AsyncClient",
    "AsyncHTTPTransport",
    "Client",
    "ConnectError",
    "ConnectTimeout",
    "HTTPStatusError",
    "HTTPTransport",
    "Headers",
    "MaxRetriesExceeded",
    "NetworkError",
    "PoolTimeout",
    "ProxyError",
    "ReadError",
    "ReadTimeout",
    "RequestError",
    "Retry",
    "RqxError",
    "Timeout",
    "TimeoutException",
    "TooManyRedirects",
    "TransportError",
    "WriteError",
    "WriteTimeout",
    "delete",
    "get",
    "head",
    "options",
    "patch",
    "post",
    "put",
    "request",
    "stream",
]
