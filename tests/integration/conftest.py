"""Everything under tests/integration talks to the local httpbin container
(`just httpbin-start`). Marked so `-m "not integration"` can skip it."""

import pytest


def pytest_collection_modifyitems(items):
    for item in items:
        if "/tests/integration/" in str(item.path):
            item.add_marker(pytest.mark.integration)
