# import from the compiled Rust extension module
from ._reqx import (
    ConnectError,
    ConnectTimeout,
    HTTPStatusError,
    MaxRetriesExceeded,
    NetworkError,
    PoolTimeout,
    ProxyError,
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

__all__ = [
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
