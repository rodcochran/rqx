"""`rqx.QueryParams` surface (https://github.com/rodcochran/rqx/issues/59).

An immutable multi-dict, mirroring httpx 0.28: values are grouped under the
first appearance of their key, mutation is copy-style, and `__setitem__` /
`update` raise rather than silently doing nothing.

The `params=` kwarg's scalar coercion lives in tests/unit/request/
test_query_params.py; this file is the class itself.
"""

import pytest

import rqx


def test_from_str():
    assert str(rqx.QueryParams("a=1&b=2")) == "a=1&b=2"


def test_from_bytes():
    assert str(rqx.QueryParams(b"a=1&b=2")) == "a=1&b=2"


def test_from_dict():
    assert str(rqx.QueryParams({"a": "1", "b": "2"})) == "a=1&b=2"


def test_from_list_of_pairs_keeps_duplicates():
    assert str(rqx.QueryParams([("a", "1"), ("a", "2")])) == "a=1&a=2"


def test_from_query_params_copies():
    original = rqx.QueryParams("a=1&a=2")
    assert rqx.QueryParams(original) == original


def test_from_none_is_empty():
    assert len(rqx.QueryParams(None)) == 0


def test_from_kwargs():
    assert str(rqx.QueryParams(a=1, b="x")) == "a=1&b=x"


def test_dict_values_are_coerced_like_the_params_kwarg():
    assert str(rqx.QueryParams({"n": 1, "on": True, "off": False})) == (
        "n=1&on=true&off=false"
    )


def test_float_keeps_python_formatting():
    assert str(rqx.QueryParams({"big": 1e16})) == "big=1e%2B16"


def test_int_beyond_i64():
    assert str(rqx.QueryParams({"n": 2**64})) == "n=18446744073709551616"


def test_a_list_value_expands_to_repeated_keys():
    assert str(rqx.QueryParams({"a": ["1", "2"]})) == "a=1&a=2"


def test_values_are_grouped_under_the_first_appearance_of_the_key():
    params = rqx.QueryParams("b=1&a=2&b=3")
    assert str(params) == "b=1&b=3&a=2"
    assert list(params.multi_items()) == [("b", "1"), ("b", "3"), ("a", "2")]


def test_get_returns_the_first_value():
    assert rqx.QueryParams("a=1&a=2").get("a") == "1"


def test_get_missing_key_returns_the_default():
    assert rqx.QueryParams("a=1").get("zz") is None
    assert rqx.QueryParams("a=1").get("zz", "d") == "d"


def test_get_list_returns_every_value():
    assert rqx.QueryParams("a=1&a=2&b=3").get_list("a") == ["1", "2"]


def test_get_list_missing_key_is_empty():
    assert rqx.QueryParams("a=1").get_list("zz") == []


def test_keys_values_items_are_one_per_key():
    params = rqx.QueryParams("a=1&a=2&b=3")
    assert list(params.keys()) == ["a", "b"]
    assert list(params.values()) == ["1", "3"]
    assert list(params.items()) == [("a", "1"), ("b", "3")]


def test_multi_items_returns_every_pair():
    assert list(rqx.QueryParams("a=1&a=2&b=3").multi_items()) == [
        ("a", "1"),
        ("a", "2"),
        ("b", "3"),
    ]


def test_getitem_returns_the_first_value():
    assert rqx.QueryParams("a=1&a=2")["a"] == "1"


def test_getitem_on_a_missing_key_raises():
    with pytest.raises(KeyError):
        rqx.QueryParams("a=1")["zz"]


def test_contains():
    params = rqx.QueryParams("a=1")
    assert "a" in params
    assert "zz" not in params


def test_iter_yields_keys():
    assert list(iter(rqx.QueryParams("a=1&a=2&b=3"))) == ["a", "b"]


def test_len_counts_unique_keys():
    assert len(rqx.QueryParams("a=1&a=2&b=3")) == 2


def test_bool():
    assert not rqx.QueryParams()
    assert rqx.QueryParams("a=1")


def test_repr():
    assert repr(rqx.QueryParams("a=1&b=2")) == "QueryParams('a=1&b=2')"


def test_eq_ignores_key_order():
    assert rqx.QueryParams("a=1&b=2") == rqx.QueryParams("b=2&a=1")


def test_eq_ignores_value_order_within_a_key():
    assert rqx.QueryParams("a=1&a=2") == rqx.QueryParams("a=2&a=1")


def test_eq_counts_duplicates():
    assert rqx.QueryParams("a=1&a=2") != rqx.QueryParams("a=1")


def test_eq_against_a_str_is_false():
    assert rqx.QueryParams("a=1") != "a=1"


def test_hash_is_consistent_with_equality():
    """Deliberate divergence: httpx hashes `str(self)`, so two params it calls
    equal can hash differently. rqx hashes what it compares."""
    assert hash(rqx.QueryParams("a=1&b=2")) == hash(rqx.QueryParams("b=2&a=1"))
    assert len({rqx.QueryParams("a=1&b=2"), rqx.QueryParams("b=2&a=1")}) == 1


def test_set_replaces_every_value_for_the_key():
    assert str(rqx.QueryParams("a=1&a=2&b=3").set("a", "9")) == "a=9&b=3"


def test_set_appends_a_new_key():
    assert str(rqx.QueryParams("a=1").set("b", "2")) == "a=1&b=2"


def test_add_keeps_the_existing_values():
    assert str(rqx.QueryParams("a=1&a=2&b=3").add("a", "9")) == "a=1&a=2&a=9&b=3"


def test_remove_drops_every_value_for_the_key():
    assert str(rqx.QueryParams("a=1&a=2&b=3").remove("a")) == "b=3"


def test_remove_of_a_missing_key_is_a_no_op():
    assert str(rqx.QueryParams("a=1").remove("zz")) == "a=1"


def test_merge_appends_and_overwrites():
    assert str(rqx.QueryParams("a=1&b=2").merge({"b": "9", "c": "3"})) == "a=1&b=9&c=3"


def test_merge_none_is_a_no_op():
    assert str(rqx.QueryParams("a=1").merge(None)) == "a=1"


def test_copy_style_mutation_leaves_the_original_alone():
    params = rqx.QueryParams("a=1")
    params.set("a", "9")
    params.add("b", "2")
    params.remove("a")
    assert str(params) == "a=1"


def test_setitem_raises_immutable():
    params = rqx.QueryParams("a=1")
    with pytest.raises(RuntimeError, match="immutable"):
        params["a"] = "2"


def test_update_raises_immutable():
    params = rqx.QueryParams("a=1")
    with pytest.raises(RuntimeError, match="immutable"):
        params.update({"a": "2"})


def test_values_are_urlencoded():
    assert str(rqx.QueryParams({"q": "a b&c"})) == "q=a+b%26c"


def test_non_ascii_values_are_utf8_percent_encoded():
    assert str(rqx.QueryParams({"q": "héllo"})) == "q=h%C3%A9llo"


def test_blank_value_keeps_the_key():
    assert str(rqx.QueryParams({"e": ""})) == "e="
