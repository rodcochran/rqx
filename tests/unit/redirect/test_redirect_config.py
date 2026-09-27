import pytest
import rqx

# ----- raise_on_redirect -----


def test_raise_on_redirect_true_raises_on_loop(flaky_server):
    """Default: raise_on_redirect=True → TooManyRedirects on loop."""
    retries = rqx.Retry()
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, follow_redirects=True, max_redirects=2)
    with pytest.raises(rqx.TooManyRedirects):
        client.get(f"{flaky_server}/redirect-loop")


def test_raise_on_redirect_false_returns_3xx(flaky_server):
    """raise_on_exceeded=False → return the last 3xx response."""
    redirects = rqx.RedirectPolicy(
        follow=True,
        max_redirects=2,
        raise_on_exceeded=False,
    )
    retries = rqx.Retry()
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, redirects=redirects)
    resp = client.get(f"{flaky_server}/redirect-loop")
    assert 300 <= resp.status_code < 400


def test_raise_on_redirect_false_raises(flaky_server):
    """raise_on_exceeded=True → raise rqx.TooManyRedirects."""
    redirects = rqx.RedirectPolicy(
        follow=True,
        max_redirects=2,
        raise_on_exceeded=True,
    )
    retries = rqx.Retry()
    transport = rqx.HTTPTransport(retries=retries)
    client = rqx.Client(transport=transport, redirects=redirects)
    with pytest.raises(rqx.TooManyRedirects):
        client.get(f"{flaky_server}/redirect-loop")


@pytest.mark.asyncio
async def test_raise_on_redirect_true_raises_on_loop_async(flaky_server):
    """Default: raise_on_redirect=True → TooManyRedirects on loop."""
    retries = rqx.Retry()
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(
        transport=transport,
        follow_redirects=True,
        max_redirects=2,
    )
    with pytest.raises(rqx.TooManyRedirects):
        await client.get(f"{flaky_server}/redirect-loop")


@pytest.mark.asyncio
async def test_raise_on_redirect_false_returns_3xx_async(flaky_server):
    """raise_on_exceeded=False → return the last 3xx response."""
    redirects = rqx.RedirectPolicy(
        follow=True,
        max_redirects=2,
        raise_on_exceeded=False,
    )
    retries = rqx.Retry()
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(transport=transport, redirects=redirects)
    resp = await client.get(f"{flaky_server}/redirect-loop")
    assert 300 <= resp.status_code < 400


@pytest.mark.asyncio
async def test_raise_on_redirect_false_raises_async(flaky_server):
    """raise_on_exceeded=True → raise rqx.TooManyRedirects."""
    redirects = rqx.RedirectPolicy(
        follow=True,
        max_redirects=2,
        raise_on_exceeded=True,
    )
    retries = rqx.Retry()
    transport = rqx.AsyncHTTPTransport(retries=retries)
    client = rqx.AsyncClient(transport=transport, redirects=redirects)
    with pytest.raises(rqx.TooManyRedirects):
        await client.get(f"{flaky_server}/redirect-loop")
