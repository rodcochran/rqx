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
    PyRetry,
    ReadError,
    ReadTimeout,
    RqxError,
    TimeoutException,
    TooManyRedirects,
    TransportError,
    WriteError,
    WriteTimeout,
)

# optional: nicer names (drop Py prefix)
Client = PyClient
AsyncClient = PyAsyncClient
Retry = PyRetry

__all__ = [
    "AsyncClient",
    "AsyncHTTPTransport",
    "Client",
    "ConnectError",
    "ConnectTimeout",
    "HTTPStatusError",
    "HTTPTransport",
    "MaxRetriesExceeded",
    "NetworkError",
    "PoolTimeout",
    "ProxyError",
    "ReadError",
    "ReadTimeout",
    "Retry",
    "RqxError",
    "TimeoutException",
    "TooManyRedirects",
    "TransportError",
    "WriteError",
    "WriteTimeout",
]
