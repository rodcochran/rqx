"""Same test body, two clients (https://github.com/rodcochran/rqx/issues/42).

Every test here takes `lib` and runs once against httpx and once against rqx,
asserting the same expected value for both. Where rqx knowingly diverges, the
test carries `@pytest.mark.rqx_diverges(issue=..., reason=...)`, which becomes
a *strict* xfail for the rqx case only: the day rqx agrees with httpx, that
test fails until the mark is removed, so flipping a divergence is deliberate.
"""

from dataclasses import dataclass
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


def pytest_collection_modifyitems(items):
    here = __file__.rsplit("/", 1)[0]
    for item in items:
        if str(item.path).startswith(here):
            item.add_marker(pytest.mark.equivalence)
