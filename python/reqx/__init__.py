# import from the compiled Rust extension module
from ._reqx import (
    ConnectError,
    ConnectTimeout,
    HTTPStatusError,
    MaxRetriesExceeded,
    NetworkError,
    PoolTimeout,
    ProxyError,
    PyAsyncClient,
    PyClient,
    ReadError,
    ReadTimeout,
    ReqxError,
    TimeoutException,
    TooManyRedirects,
    TransportError,
    WriteError,
    WriteTimeout,
)

# optional: nicer names (drop Py prefix)
Client = PyClient
AsyncClient = PyAsyncClient

__all__ = [
    "AsyncClient",
    "Client",
    "ConnectError",
    "ConnectTimeout",
    "HTTPStatusError",
    "MaxRetriesExceeded",
    "NetworkError",
    "PoolTimeout",
    "ProxyError",
    "ReadError",
    "ReadTimeout",
    "ReqxError",
    "TimeoutException",
    "TooManyRedirects",
    "TransportError",
    "WriteError",
    "WriteTimeout",
]
