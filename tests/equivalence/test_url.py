"""`URL` / `QueryParams` parity with httpx 0.28
(https://github.com/rodcochran/rqx/issues/59).

Pure object tests — no server. The httpx run is the oracle: every assertion
here is a behavior httpx exhibits today, so anything rqx gets wrong shows up
as a one-sided failure.
"""

import httpx
import pytest

ISSUE_59 = "https://github.com/rodcochran/rqx/issues/59"

FULL = "https://user:pass@example.com:8443/a/b?x=1&y=2#frag"


def test_components(lib):
    url = lib.module.URL(FULL)
    assert (url.scheme, url.username, url.password) == ("https", "user", "pass")
    assert (url.host, url.port, url.path) == ("example.com", 8443, "/a/b")
    assert (url.query, url.fragment) == (b"x=1&y=2", "frag")
    assert url.raw_path == b"/a/b?x=1&y=2"


def test_default_port_normalizes_to_none(lib):
    assert lib.module.URL("http://example.com:80/a").port is None
    assert lib.module.URL("https://example.com:443/a").port is None
    assert str(lib.module.URL("http://example.com:80/a")) == "http://example.com/a"


def test_path_is_decoded_and_raw_path_is_not(lib):
    url = lib.module.URL("http://example.com/a b")
    assert url.path == "/a b"
    assert url.raw_path == b"/a%20b"


def test_absolute_and_relative(lib):
    assert lib.module.URL("http://example.com/a").is_absolute_url
    assert lib.module.URL("/a/b").is_relative_url


@pytest.mark.parametrize(
    "reference", ["c", "/a b", "a/b?x=1#f", "//other.com/a", "?x=1", "#f", "", "../d"]
)
def test_relative_references_keep_their_shape(lib, reference):
    """A relative reference is echoed as written — dot segments and all — with
    only the characters RFC 3986 rejects percent-encoded."""
    url = lib.module.URL(reference)
    assert str(url) == str(httpx.URL(reference))
    assert (url.path, url.raw_path, url.query, url.fragment) == (
        httpx.URL(reference).path,
        httpx.URL(reference).raw_path,
        httpx.URL(reference).query,
        httpx.URL(reference).fragment,
    )
    assert url.is_relative_url


@pytest.mark.parametrize(
    ("base", "reference"),
    [
        ("/a/b", "c"),
        ("a/b", "c"),
        ("a/b", "/c"),
        ("a/b", "../c"),
        ("/a", "//host/x"),
        ("//h/a", "b"),
        ("//h/a", "/x"),
        ("/a/b", "/c/../d"),
        ("a/b", "?q=1"),
        ("a/b", "#f"),
        ("/a/b", ""),
        ("/a/b?x=1", ""),
        ("/a/b?x=1", "#f"),
        ("/a/b?x=1", "c"),
        ("/a/b?x=1", "?y=2"),
        ("//h/a?x=1", ""),
        ("a/b?x=1", ""),
        ("/a#f1", ""),
        ("http://x/a#f1", ""),
        ("", "c"),
        ("/a", "https://o/z"),
    ],
)
def test_join_from_a_relative_base(lib, base, reference):
    """RFC 3986 §5.3: an argument with its own scheme or authority replaces the
    base; otherwise the base's authority and path shape survive the merge."""
    assert str(lib.module.URL(base).join(reference)) == str(
        httpx.URL(base).join(reference)
    )


def test_copy_with(lib):
    url = lib.module.URL(FULL).copy_with(host="other.com", port=None, path="/z")
    assert str(url) == "https://user:pass@other.com/z?x=1&y=2#frag"


def test_copy_param_helpers(lib):
    url = lib.module.URL("http://example.com/a?x=1&y=2")
    assert str(url.copy_set_param("x", "9")) == "http://example.com/a?x=9&y=2"
    assert str(url.copy_add_param("x", "9")) == "http://example.com/a?x=1&x=9&y=2"
    assert str(url.copy_remove_param("x")) == "http://example.com/a?y=2"
    assert str(url.copy_merge_params({"z": "3"})) == "http://example.com/a?x=1&y=2&z=3"


def test_join(lib):
    url = lib.module.URL("https://example.com/a/b")
    assert str(url.join("c")) == "https://example.com/a/c"
    assert str(url.join("../d")) == "https://example.com/d"
    assert str(url.join("https://other.com/z")) == "https://other.com/z"


def test_repr_masks_the_password(lib):
    assert repr(lib.module.URL(FULL)) == (
        "URL('https://user:[secure]@example.com:8443/a/b?x=1&y=2#frag')"
    )


def test_eq_accepts_a_str(lib):
    assert lib.module.URL("http://example.com/a") == "http://example.com/a"


def test_idna_host(lib):
    url = lib.module.URL("http://ünicode.com/a")
    assert url.host == "ünicode.com"
    assert str(url) == "http://xn--nicode-2ya.com/a"


def test_query_params_group_values_under_the_first_key(lib):
    params = lib.module.QueryParams("b=1&a=2&b=3")
    assert str(params) == "b=1&b=3&a=2"
    assert list(params.multi_items()) == [("b", "1"), ("b", "3"), ("a", "2")]


def test_query_params_accessors(lib):
    params = lib.module.QueryParams("a=1&a=2&b=3")
    assert params["a"] == "1"
    assert params.get_list("a") == ["1", "2"]
    assert list(params.items()) == [("a", "1"), ("b", "3")]
    assert len(params) == 2


def test_query_params_copy_helpers(lib):
    params = lib.module.QueryParams("a=1&a=2&b=3")
    assert str(params.set("a", "9")) == "a=9&b=3"
    assert str(params.add("a", "9")) == "a=1&a=2&a=9&b=3"
    assert str(params.remove("a")) == "b=3"
    assert str(params.merge({"c": "4"})) == "a=1&a=2&b=3&c=4"


def test_query_params_are_immutable(lib):
    params = lib.module.QueryParams("a=1")
    with pytest.raises(RuntimeError, match="immutable"):
        params["a"] = "2"
    with pytest.raises(RuntimeError, match="immutable"):
        params.update({"a": "2"})


def test_query_params_scalar_coercion(lib):
    assert str(lib.module.QueryParams({"n": 1, "on": True, "off": False})) == (
        "n=1&on=true&off=false"
    )
    assert str(lib.module.QueryParams({"big": 1e16})) == "big=1e%2B16"


def test_none_param_value_is_sent_as_an_empty_value(lib):
    """rqx drops the key today (https://github.com/rodcochran/rqx/issues/115);
    httpx sends `a=`. Decision for #59: rqx follows httpx."""
    assert str(lib.module.QueryParams({"a": None, "b": 1})) == "a=&b=1"


def test_params_replace_a_query_already_on_the_url(lib):
    """rqx appends today; httpx replaces. Same decision as above."""
    assert str(lib.module.URL("http://example.com/a?keep=1", params={"a": 2})) == (
        "http://example.com/a?a=2"
    )


@pytest.mark.rqx_diverges(
    issue=ISSUE_59,
    reason="rqx is backed by `url::Url`, which always normalizes an empty path to `/`",
)
def test_an_empty_path_is_not_normalized(lib):
    assert str(lib.module.URL("http://example.com")) == "http://example.com"
