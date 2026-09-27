import rqx

# ----- raise_on_redirect -----


def test_redirect_getters():
    expected_follow_val = True
    expected_max_redirects_val = 3
    expected_raise_on_exceeded_val = True

    redirects = rqx.RedirectPolicy(
        follow=expected_follow_val,
        max_redirects=expected_max_redirects_val,
        raise_on_exceeded=expected_raise_on_exceeded_val,
    )

    assert redirects.follow == expected_follow_val
    assert redirects.max_redirects == expected_max_redirects_val
    assert redirects.raise_on_exceeded == expected_raise_on_exceeded_val


def test_redirect_repr():

    expected_follow_val = True
    expected_max_redirects_val = 3
    expected_raise_on_exceeded_val = True
    expected_repr_val = f"RedirectPolicy(follow={expected_follow_val}, max_redirects={expected_max_redirects_val}, raise_on_exceeded={expected_raise_on_exceeded_val})"

    redirects = rqx.RedirectPolicy(
        follow=expected_follow_val,
        max_redirects=expected_max_redirects_val,
        raise_on_exceeded=expected_raise_on_exceeded_val,
    )

    assert repr(redirects) == expected_repr_val
