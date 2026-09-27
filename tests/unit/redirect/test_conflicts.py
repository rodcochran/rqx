import pytest
import rqx


def test_raise_on_follow_redirect_config_conflict():
    """Redirect policy, follow = True + client-level config = False -> should raise"""
    redirects = rqx.RedirectPolicy(
        follow=True,
    )
    with pytest.raises(rqx.RqxError):
        rqx.Client(
            follow_redirects=False,
            redirects=redirects,
        )


def test_raise_on_max_redirects_config_conflict():
    """Redirect policy, max = 2 + client-level config = 3 -> should raise"""
    redirects = rqx.RedirectPolicy(max_redirects=2)
    with pytest.raises(rqx.RqxError):
        rqx.Client(
            max_redirects=3,
            redirects=redirects,
        )


def test_agreeing_redirect_config_is_accepted():
    """Redirect policy and client-level kwargs with the same values -> no conflict"""
    redirects = rqx.RedirectPolicy(follow=True, max_redirects=3)
    client = rqx.Client(
        follow_redirects=True,
        max_redirects=3,
        redirects=redirects,
    )
    assert client.redirects.follow is True
    assert client.redirects.max_redirects == 3


@pytest.mark.asyncio
async def test_raise_on_follow_redirect_config_conflict_async():
    """Redirect policy, follow = True + client-level config = False -> should raise"""
    redirects = rqx.RedirectPolicy(
        follow=True,
    )
    with pytest.raises(rqx.RqxError):
        rqx.AsyncClient(
            follow_redirects=False,
            redirects=redirects,
        )


@pytest.mark.asyncio
async def test_raise_on_max_redirects_config_conflict_async():
    """Redirect policy, max = 2 + client-level config = 3 -> should raise"""
    redirects = rqx.RedirectPolicy(max_redirects=2)
    with pytest.raises(rqx.RqxError):
        rqx.AsyncClient(
            max_redirects=3,
            redirects=redirects,
        )


@pytest.mark.asyncio
async def test_agreeing_redirect_config_is_accepted_async():
    """Redirect policy and client-level kwargs with the same values -> no conflict"""
    redirects = rqx.RedirectPolicy(follow=True, max_redirects=3)
    client = rqx.AsyncClient(
        follow_redirects=True,
        max_redirects=3,
        redirects=redirects,
    )
    assert client.redirects.follow is True
    assert client.redirects.max_redirects == 3
