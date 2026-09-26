use pyo3::prelude::{PyResult, pyclass, pymethods};

use rqx_core::redirect::RedirectPolicy;

#[pyclass(name = "RedirectPolicy", from_py_object)]
#[derive(Clone)]
pub struct PyRedirectPolicy {
    pub(crate) inner: RedirectPolicy,
}

impl PyRedirectPolicy {
    pub fn new(inner: RedirectPolicy) -> Self {
        Self { inner }
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
    ) -> PyResult<Self> {
        Ok(Self {
            inner: RedirectPolicy::with_defaults(follow, max_redirects, raise_on_exceeded),
        })
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
