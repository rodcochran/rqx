"""`repr()` of rqx config objects spells every field the way Python's own
`repr()` would, for any value the constructor accepts."""

from hypothesis import given
from hypothesis import strategies as st

import rqx

# Every float, including nan, +/-inf, -0.0 and subnormals.
any_float = st.floats()
phase = st.one_of(st.none(), any_float)


@given(value=any_float)
def test_timeout_single_value_repr(value):
    expected = (
        f"Timeout(connect={value!r}, read={value!r}, write={value!r}, pool={value!r})"
    )
    assert repr(rqx.Timeout(value)) == expected


@given(connect=phase, read=phase, write=phase, pool=phase)
def test_timeout_per_phase_repr(connect, read, write, pool):
    timeout = rqx.Timeout(connect=connect, read=read, write=write, pool=pool)
    expected = (
        f"Timeout(connect={connect!r}, read={read!r}, write={write!r}, pool={pool!r})"
    )
    assert repr(timeout) == expected


@given(
    follow=st.booleans(),
    max_redirects=st.integers(min_value=0, max_value=2**32 - 1),
    raise_on_exceeded=st.booleans(),
)
def test_redirect_policy_repr(follow, max_redirects, raise_on_exceeded):
    policy = rqx.RedirectPolicy(
        follow=follow,
        max_redirects=max_redirects,
        raise_on_exceeded=raise_on_exceeded,
    )
    expected = (
        f"RedirectPolicy(follow={follow!r}, max_redirects={max_redirects!r}, "
        f"raise_on_exceeded={raise_on_exceeded!r})"
    )
    assert repr(policy) == expected
