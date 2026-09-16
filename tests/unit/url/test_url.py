"""`rqx.URL` surface (https://github.com/rodcochran/rqx/issues/59).

Mirrors httpx 0.28's URL: read-only component properties, copy-style
mutation, and equality against plain strings so `resp.url == "..."` keeps
working once `response.url` stops being a `str`.
"""

import pytest

import rqx

FULL = "https://user:pass@example.com:8443/a/b?x=1&y=2#frag"


def test_components_of_a_full_url():
    url = rqx.URL(FULL)
    assert url.scheme == "https"
    assert url.username == "user"
    assert url.password == "pass"
    assert url.host == "example.com"
    assert url.port == 8443
    assert url.path == "/a/b"
    assert url.fragment == "frag"


def test_missing_components_are_empty_strings():
    url = rqx.URL("http://example.com/a")
    assert url.username == ""
    assert url.password == ""
    assert url.fragment == ""
    assert url.query == b""


def test_query_is_bytes():
    assert rqx.URL(FULL).query == b"x=1&y=2"


def test_params_is_a_query_params():
    params = rqx.URL(FULL).params
    assert isinstance(params, rqx.QueryParams)
    assert params["x"] == "1"


def test_raw_path_carries_the_query():
    assert rqx.URL(FULL).raw_path == b"/a/b?x=1&y=2"


def test_raw_path_of_a_bare_host_is_the_root():
    assert rqx.URL("http://example.com/").raw_path == b"/"


def test_path_is_percent_decoded_but_raw_path_is_not():
    url = rqx.URL("http://example.com/a b")
    assert url.path == "/a b"
    assert url.raw_path == b"/a%20b"


def test_default_http_port_normalizes_to_none():
    assert rqx.URL("http://example.com:80/a").port is None


def test_default_https_port_normalizes_to_none():
    assert rqx.URL("https://example.com:443/a").port is None


def test_default_port_is_dropped_from_the_string_form():
    assert str(rqx.URL("http://example.com:80/a")) == "http://example.com/a"


def test_non_default_port_is_kept():
    url = rqx.URL("http://example.com:8080/a")
    assert url.port == 8080
    assert str(url) == "http://example.com:8080/a"


def test_absolute_and_relative_flags():
    assert rqx.URL("http://example.com/a").is_absolute_url
    assert not rqx.URL("http://example.com/a").is_relative_url
    assert rqx.URL("/a/b").is_relative_url
    assert not rqx.URL("/a/b").is_absolute_url


def test_relative_url_keeps_its_path_and_query():
    url = rqx.URL("/a/b?c=1")
    assert url.host == ""
    assert url.path == "/a/b"
    assert url.query == b"c=1"


def test_empty_url_is_relative():
    url = rqx.URL()
    assert str(url) == ""
    assert url.is_relative_url


def test_host_is_lowercased():
    assert rqx.URL("HTTP://Example.COM/A").host == "example.com"


def test_scheme_is_lowercased_but_path_case_is_kept():
    url = rqx.URL("HTTP://example.com/A")
    assert url.scheme == "http"
    assert url.path == "/A"


def test_host_is_the_unicode_form_but_the_wire_form_is_punycode():
    url = rqx.URL("http://ünicode.com/a")
    assert url.host == "ünicode.com"
    assert str(url) == "http://xn--nicode-2ya.com/a"


def test_copy_constructor_from_url():
    original = rqx.URL(FULL)
    assert rqx.URL(original) == original


def test_url_argument_with_kwargs_is_copy_with():
    assert rqx.URL(rqx.URL("http://example.com/a"), path="/b") == "http://example.com/b"


def test_kwargs_only_construction():
    url = rqx.URL(scheme="https", host="example.com", path="/p", params={"a": 1})
    assert str(url) == "https://example.com/p?a=1"


def test_copy_with_replaces_components():
    url = rqx.URL(FULL).copy_with(host="other.com", port=None, path="/z")
    assert str(url) == "https://user:pass@other.com/z?x=1&y=2#frag"


def test_copy_with_none_drops_userinfo():
    assert rqx.URL("http://u:p@example.com/a").copy_with(
        username=None, password=None
    ) == rqx.URL("http://example.com/a")


def test_copy_with_query_takes_bytes():
    assert rqx.URL("http://example.com/a?z=1").copy_with(query=b"a=2") == (
        "http://example.com/a?a=2"
    )


def test_copy_with_params_replaces_the_whole_query():
    assert rqx.URL("http://example.com/a?z=1").copy_with(params={"a": 2}) == (
        "http://example.com/a?a=2"
    )


def test_copy_with_rejects_an_unknown_component():
    with pytest.raises(TypeError):
        rqx.URL("http://example.com/a").copy_with(nope=1)


def test_copy_set_param_replaces_every_value_for_the_key():
    assert rqx.URL("http://example.com/a?x=1&x=2&y=3").copy_set_param("x", "9") == (
        "http://example.com/a?x=9&y=3"
    )


def test_copy_add_param_keeps_the_existing_values():
    assert rqx.URL("http://example.com/a?x=1&y=2").copy_add_param("x", "9") == (
        "http://example.com/a?x=1&x=9&y=2"
    )


def test_copy_remove_param_drops_the_key():
    assert rqx.URL("http://example.com/a?x=1&y=2").copy_remove_param("x") == (
        "http://example.com/a?y=2"
    )


def test_copy_merge_params_appends_new_keys():
    assert rqx.URL("http://example.com/a?x=1").copy_merge_params({"z": "3"}) == (
        "http://example.com/a?x=1&z=3"
    )


def test_copy_merge_params_overwrites_a_shared_key():
    assert rqx.URL("http://example.com/a?x=1").copy_merge_params({"x": "9"}) == (
        "http://example.com/a?x=9"
    )


def test_copy_leaves_the_original_alone():
    url = rqx.URL("http://example.com/a?x=1")
    url.copy_set_param("x", "9")
    assert str(url) == "http://example.com/a?x=1"


def test_join_resolves_a_relative_reference():
    assert rqx.URL("https://example.com/a/b").join("c") == "https://example.com/a/c"


def test_join_resolves_dot_segments():
    assert (
        rqx.URL("https://example.com/a/b/c").join("../d") == "https://example.com/a/d"
    )


def test_join_with_an_absolute_url_replaces_everything():
    assert rqx.URL("https://example.com/a").join("https://other.com/z") == (
        "https://other.com/z"
    )


def test_join_accepts_a_url():
    assert rqx.URL("https://example.com/a/b").join(rqx.URL("c")) == (
        "https://example.com/a/c"
    )


def test_str_round_trips():
    assert str(rqx.URL(FULL)) == FULL


def test_repr_masks_the_password():
    assert repr(rqx.URL(FULL)) == (
        "URL('https://user:[secure]@example.com:8443/a/b?x=1&y=2#frag')"
    )


def test_repr_without_a_password_is_the_plain_url():
    assert repr(rqx.URL("http://example.com/a")) == "URL('http://example.com/a')"


def test_eq_accepts_a_str():
    assert rqx.URL("http://example.com/a") == "http://example.com/a"


def test_eq_against_an_unrelated_type_is_false():
    assert rqx.URL("http://example.com/a") != 3


def test_equal_urls_hash_equal():
    a, b = rqx.URL("http://example.com/a"), rqx.URL("http://example.com/a")
    assert hash(a) == hash(b)
    assert len({a, b}) == 1


def test_usable_as_a_dict_key():
    assert {rqx.URL("http://example.com/a"): 1}[rqx.URL("http://example.com/a")] == 1


def test_invalid_url_raises_invalid_url():
    """httpx raises `httpx.InvalidURL(Exception)`; rqx keeps the name but puts
    it under ValueError too, so both `except` clauses port."""
    with pytest.raises(rqx.InvalidURL):
        rqx.URL("http://[::1")
    with pytest.raises(ValueError):
        rqx.URL("http://example.com:notaport/")


def test_non_str_url_raises_type_error():
    with pytest.raises(TypeError):
        rqx.URL(123)
