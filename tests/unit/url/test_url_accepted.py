"""`rqx.URL` and `rqx.QueryParams` are accepted wherever a str URL or a params
mapping is today, and `response.url` hands one back
(https://github.com/rodcochran/rqx/issues/59).

The breaking half of the issue: `response.url` stops being a `str`. Equality
against a string still works, so `resp.url == "..."` code is unaffected;
`resp.url.endswith(...)` needs `str(resp.url)`.
"""

import pytest

import rqx


def test_response_url_is_a_url(flaky_server):
    resp = rqx.get(rqx.URL(f"{flaky_server}/echo-url/x"))
    assert isinstance(resp.url, rqx.URL)


def test_response_url_compares_equal_to_the_string_it_was_given(flaky_server):
    url = f"{flaky_server}/echo-url/x"
    assert rqx.get(url).url == url


def test_response_url_exposes_components(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-url/x", params={"page": 2})
    assert resp.url.path == "/echo-url/x"
    assert resp.url.params["page"] == "2"


def test_client_accepts_a_url_for_a_request(flaky_server):
    with rqx.Client() as client:
        echoed = client.get(rqx.URL(f"{flaky_server}/echo-url/x")).json()
    assert echoed["path"] == "/echo-url/x"


def test_client_accepts_a_url_as_base_url(flaky_server):
    with rqx.Client(base_url=rqx.URL(f"{flaky_server}/echo-url/")) as client:
        assert client.get("sub").json()["path"] == "/echo-url/sub"


def test_client_base_url_is_a_url(flaky_server):
    with rqx.Client(base_url=flaky_server) as client:
        assert isinstance(client.base_url, rqx.URL)
        assert client.base_url == f"{flaky_server}/"


def test_a_network_relative_url_does_not_retarget_the_host(flaky_server):
    """`//other.example/x` carries an authority, but a relative request URL
    contributes its path and query only — the base's host still gets it, as
    in httpx."""
    with rqx.Client(base_url=f"{flaky_server}/echo-url/") as client:
        echoed = client.get("//other.example/sub").json()
    assert echoed["path"] == "/echo-url/sub"


def test_url_query_survives_the_request(flaky_server):
    url = rqx.URL(f"{flaky_server}/echo-url/x").copy_set_param("a", "1")
    assert rqx.get(url).json()["query"] == "a=1"


@pytest.mark.asyncio
async def test_async_client_accepts_a_url(flaky_server):
    async with rqx.AsyncClient() as client:
        resp = await client.get(rqx.URL(f"{flaky_server}/echo-url/x"))
    assert resp.json()["path"] == "/echo-url/x"
    assert isinstance(resp.url, rqx.URL)


def test_params_accepts_query_params(flaky_server):
    params = rqx.QueryParams("a=1&a=2&b=3")
    assert rqx.get(f"{flaky_server}/echo-url/p", params=params).json()["query"] == (
        "a=1&a=2&b=3"
    )


def test_params_accepts_a_list_of_pairs(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-url/p", params=[("a", "1"), ("a", "2")])
    assert resp.json()["query"] == "a=1&a=2"


def test_params_accepts_a_str(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-url/p", params="a=1&b=2")
    assert resp.json()["query"] == "a=1&b=2"


def test_params_accepts_bytes(flaky_server):
    resp = rqx.get(f"{flaky_server}/echo-url/p", params=b"a=1&b=2")
    assert resp.json()["query"] == "a=1&b=2"


def test_stream_response_url_is_a_url(flaky_server):
    with rqx.stream("GET", f"{flaky_server}/streamable") as resp:
        assert isinstance(resp.url, rqx.URL)
