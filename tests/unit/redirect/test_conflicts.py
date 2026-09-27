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
