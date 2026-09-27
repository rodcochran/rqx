use pyo3::prelude::{pyclass, pymethods};

use rqx_core::redirect::RedirectPolicy;

use crate::exceptions::{PyRqxError, RqxError};
use crate::py_utils::repr::PyRepr;

#[pyclass(name = "RedirectPolicy", from_py_object, module = "rqx", frozen)]
#[derive(Clone)]
pub struct PyRedirectPolicy {
    pub(crate) inner: RedirectPolicy,
}

impl PyRedirectPolicy {
    pub fn new(inner: RedirectPolicy) -> Self {
        Self { inner }
    }

    pub fn valid_policy_from_options(
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        redirects: Option<PyRedirectPolicy>,
    ) -> Result<RedirectPolicy, PyRqxError> {
        match redirects {
            Some(r) => {
                if let Some(mr) = max_redirects {
                    if r.max_redirects() != mr {
                        return Err(RqxError::new_err(
                            "Cannot specify conflicting max_redirects and RedirectPolicy.max_redirects",
                        )
                        .into());
                    }
                }
                if let Some(fr) = follow_redirects {
                    if r.follow() != fr {
                        return Err(RqxError::new_err(
                            "Cannot specify conflicting follow_redirects and RedirectPolicy.follow",
                        )
                        .into());
                    }
                }
                Ok(r.inner)
            }
            None => Ok(RedirectPolicy::with_defaults(
                follow_redirects,
                max_redirects,
                None,
            )),
        }
    }
}

#[pymethods]
impl PyRedirectPolicy {
    #[new]
    #[pyo3(signature = (
        follow=None,
        max_redirects=None,
        raise_on_exceeded=None,
    ))]
    fn __new__(
        follow: Option<bool>,
        max_redirects: Option<u32>,
        raise_on_exceeded: Option<bool>,
    ) -> Self {
        Self {
            inner: RedirectPolicy::with_defaults(follow, max_redirects, raise_on_exceeded),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "RedirectPolicy(follow={}, max_redirects={}, raise_on_exceeded={})",
            self.inner.follow.py_repr(),
            self.inner.max_redirects.py_repr(),
            self.inner.raise_on_exceeded.py_repr(),
        )
    }

    // whether to follow redirects
    #[getter]
    pub fn follow(&self) -> bool {
        self.inner.follow
    }

    // max redirects
    #[getter]
    pub fn max_redirects(&self) -> u32 {
        self.inner.max_redirects
    }

    // whether to raise on max redirects
    #[getter]
    pub fn raise_on_exceeded(&self) -> bool {
        self.inner.raise_on_exceeded
    }
}
