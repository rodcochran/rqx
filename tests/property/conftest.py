"""Hypothesis budget. `HYPOTHESIS_PROFILE=nightly` runs ten times the examples.

Every property here makes a real request to the fixture server, so the
per-example deadline is off: hypothesis's 200 ms default would flake under
xdist for reasons unrelated to the code under test.
"""

import os

from hypothesis import settings

settings.register_profile("default", max_examples=100, deadline=None)
settings.register_profile("nightly", max_examples=1000, deadline=None)
settings.load_profile(os.getenv("HYPOTHESIS_PROFILE", "default"))
