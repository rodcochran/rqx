"""Same test body, two clients (https://github.com/rodcochran/rqx/issues/42).

Every test here takes `lib` and runs once against httpx and once against rqx,
asserting the same expected value for both. Where rqx knowingly diverges, the
test carries `@pytest.mark.rqx_diverges(issue=..., reason=...)`, which becomes
a *strict* xfail for the rqx case only: the day rqx agrees with httpx, that
test fails until the mark is removed, so flipping a divergence is deliberate.
"""

from dataclasses import dataclass
from pathlib import Path
from types import ModuleType

import httpx
import pytest
import rqx


@dataclass(frozen=True)
class Lib:
    name: str
    module: ModuleType

    def client(self, **kwargs):
        return self.module.Client(**kwargs)

    def async_client(self, **kwargs):
        return self.module.AsyncClient(**kwargs)

    def proxied_client(self, proxy_url: str):
        proxy = proxy_url.rstrip("/")
        if self.name == "httpx":
            return self.module.Client(proxy=proxy)
        transport = self.module.HTTPTransport(proxy={"http": proxy, "https": proxy})
        return self.module.Client(transport=transport)


@pytest.fixture(params=["httpx", "rqx"])
def lib(request):
    which = Lib(request.param, {"httpx": httpx, "rqx": rqx}[request.param])
    mark = request.node.get_closest_marker("rqx_diverges")
    if mark and which.name == "rqx":
        request.applymarker(
            pytest.mark.xfail(
                strict=True, reason=f"{mark.kwargs['reason']} ({mark.kwargs['issue']})"
            )
        )
    return which


HERE = Path(__file__).parent

PROXY_VARS = ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY")


@pytest.fixture(autouse=True)
def _no_ambient_proxy(monkeypatch):
    """Both clients honor proxy env vars at construction; the failure-mode
    tests must see the network directly."""
    for name in PROXY_VARS:
        monkeypatch.delenv(name, raising=False)
        monkeypatch.delenv(name.lower(), raising=False)


def pytest_collection_modifyitems(items):
    for item in items:
        if HERE in item.path.parents:
            item.add_marker(pytest.mark.equivalence)
