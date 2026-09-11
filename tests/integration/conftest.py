"""Everything under tests/integration talks to the local httpbin container
(`just httpbin-start`). Marked so `-m "not integration"` can skip it."""

from pathlib import Path

import pytest

HERE = Path(__file__).parent


def pytest_collection_modifyitems(items):
    for item in items:
        if HERE in item.path.parents:
            item.add_marker(pytest.mark.integration)
