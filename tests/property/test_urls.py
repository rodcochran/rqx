"""base_url + relative path resolve the way httpx merges them: base path
segments are kept, a leading slash on the relative path doesn't reset to the
root, and an absolute URL ignores the base entirely."""

from hypothesis import given
from hypothesis import strategies as st

import rqx
from tests.property.strategies import query_pairs, segments


def _expected_path(base_segs, rel_segs):
    path = "/echo-url/" + "/".join(base_segs)
    if base_segs:
        path += "/"
    return path + "/".join(rel_segs)


@given(segments, st.booleans(), segments, st.booleans(), query_pairs)
def test_relative_paths_join_under_the_base_path(
    flaky_server, base_segs, base_trailing_slash, rel_segs, rel_leading_slash, pairs
):
    base = f"{flaky_server}/echo-url/" + "/".join(base_segs)
    if base_trailing_slash and base_segs:
        base += "/"
    rel = ("/" if rel_leading_slash else "") + "/".join(rel_segs)
    query = "&".join(f"{k}={v}" for k, v in pairs)
    if query:
        rel += "?" + query

    echoed = rqx.Client(base_url=base).get(rel).json()
    assert echoed["path"] == _expected_path(base_segs, rel_segs)
    assert echoed["query"] == query


@given(segments, segments)
def test_absolute_url_ignores_the_base(flaky_server, base_segs, abs_segs):
    base = f"{flaky_server}/echo-url/" + "/".join(base_segs)
    absolute = f"{flaky_server}/echo-url/" + "/".join(abs_segs)
    echoed = rqx.Client(base_url=base).get(absolute).json()
    assert echoed["path"] == "/echo-url/" + "/".join(abs_segs)
