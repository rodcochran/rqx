"""`rqx.__version__` is the compiled crate version, the same one the wheel carries."""

from importlib.metadata import version

import rqx


def test_version_matches_installed_distribution():
    assert rqx.__version__ == version("rqx")


def test_version_is_exported():
    assert "__version__" in rqx.__all__
