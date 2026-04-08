# import from the compiled Rust extension module
from ._reqx import PyClient

# optional: nicer names (drop Py prefix)
Client = PyClient

__all__ = [
    "Client",
]
