# _types.pyi
class PyClient:
    def __init__(
        self,
        timeout: int | None = None,
        follow_redirects: bool | None = None,
        max_redirects: int | None = None,
    ) -> None: ...
