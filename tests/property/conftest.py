"""Hypothesis budget. `HYPOTHESIS_PROFILE=nightly` runs ten times the examples.

Every property here makes a real request to the fixture server, so the
per-example deadline is off: hypothesis's 200 ms default would flake under
xdist for reasons unrelated to the code under test.
"""

import os

import pytest
import rqx
from hypothesis import settings

settings.register_profile("default", max_examples=100, deadline=None)
settings.register_profile("nightly", max_examples=1000, deadline=None)
settings.load_profile(os.getenv("HYPOTHESIS_PROFILE", "default"))


@pytest.fixture(scope="module")
def client():
    """One client per module. Building a Client loads the system CA store,
    which is tens of milliseconds on Linux; a fresh one per example was most
    of the property suite's CI time."""
    with rqx.Client() as c:
        yield c


@pytest.fixture(scope="module")
def transport():
    """For tests that need a per-example Client(base_url=...): sharing the
    transport skips the CA-store load while keeping the client cheap."""
    return rqx.HTTPTransport()
