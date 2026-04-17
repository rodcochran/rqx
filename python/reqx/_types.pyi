# _types.pyi
class PyRetry:
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
    ) -> None: ...

class HTTPTransport:
    def __init__(
        self,
        retries: PyRetry | None = None,
        max_connections: int | None = None,
        max_keepalive_connections: int | None = None,
        keepalive_expiry: float | None = None,
    ): ...

class AsyncHTTPTransport:
    def __init__(
        self,
        retries: PyRetry | None = None,
        max_connections: int | None = None,
        max_keepalive_connections: int | None = None,
        keepalive_expiry: float | None = None,
    ): ...

class PyClient:
    def __init__(
        self,
        timeout: int | None = None,
        follow_redirects: bool | None = None,
        max_redirects: int | None = None,
        transport: HTTPTransport | None = None,
    ) -> None: ...

class PyAsyncClient:
    def __init__(
        self,
        timeout: int | None = None,
        follow_redirects: bool | None = None,
        max_redirects: int | None = None,
        transport: AsyncHTTPTransport | None = None,
    ) -> None: ...
