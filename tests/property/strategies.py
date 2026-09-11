"""Input strategies shared by the property tests. Realistic by construction:
URLs are composed from parts, header names are tokens, JSON is bounded."""

from hypothesis import strategies as st

# Any str that can be UTF-8 encoded: lone surrogates can't go on a wire anywhere.
text = st.text(alphabet=st.characters(exclude_categories=["Cs"]), max_size=30)

# --- params ------------------------------------------------------------------

param_value = st.one_of(
    text,
    st.integers(),
    st.floats(allow_nan=False, allow_infinity=False),
    st.booleans(),
    st.none(),
)
params = st.dictionaries(text, param_value, max_size=8)

# --- headers -----------------------------------------------------------------

TOKEN_CHARS = (
    "!#$%&'*+-.^_`|~0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
)
NON_TOKEN_CHARS = '"(),/:;<=>?@[\\]{} '

# `x-` prefix keeps generated names clear of framing headers (content-length,
# host, ...) whose values would break the request for unrelated reasons.
header_name = st.text(alphabet=TOKEN_CHARS, min_size=1, max_size=16).map(
    lambda s: "x-" + s
)
# Printable ASCII, edges stripped: servers strip edge whitespace on receipt.
header_value = st.text(
    alphabet=st.characters(min_codepoint=0x20, max_codepoint=0x7E), max_size=40
).map(str.strip)
headers = st.dictionaries(header_name, header_value, max_size=8)


@st.composite
def header_name_with_one_bad_char(draw):
    good = draw(st.text(alphabet=TOKEN_CHARS, min_size=1, max_size=16))
    bad = draw(st.sampled_from(NON_TOKEN_CHARS))
    at = draw(st.integers(min_value=0, max_value=len(good)))
    return good[:at] + bad + good[at:]


# --- json --------------------------------------------------------------------

# Ints stay within what serde_json holds exactly; past 2^64 is a documented
# divergence (https://github.com/rodcochran/rqx/issues/116).
json_int = st.integers(min_value=-(2**63), max_value=2**64 - 1)
json_float = st.floats(allow_nan=False, allow_infinity=False)
_scalar_no_float = st.one_of(st.none(), st.booleans(), json_int, text)
_scalar = st.one_of(_scalar_no_float, json_float)


def _nest(children):
    return st.one_of(
        st.lists(children, max_size=5), st.dictionaries(text, children, max_size=5)
    )


json_value = st.recursive(_scalar, _nest, max_leaves=30)
json_value_no_floats = st.recursive(_scalar_no_float, _nest, max_leaves=30)

# --- urls --------------------------------------------------------------------

# Unreserved chars minus `.`, so no segment is ever `.` or `..` (RFC
# resolution would legitimately rewrite those and the expectation gets murky).
SEGMENT_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_~"
segment = st.text(alphabet=SEGMENT_CHARS, min_size=1, max_size=8)
segments = st.lists(segment, max_size=4)
query_pairs = st.lists(st.tuples(segment, segment), max_size=3)
