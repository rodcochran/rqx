use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyString};
use url::Url;

use crate::query_params::PyQueryParams;

use rqx_core::query_params::QueryPairs;
use rqx_core::url::BaseUrl;
use rqx_core::url::{components::UrlComponents, reference::UrlReference, url::RqxClientUrl};

#[pyclass(name = "URL", module = "rqx", frozen)]
pub struct PyURL {
    inner: RqxClientUrl,
}

impl PyURL {
    pub fn new(url: RqxClientUrl) -> Self {
        Self { inner: url }
    }

    pub fn extract_reference(obj: &Bound<'_, PyAny>) -> PyResult<UrlReference> {
        if let Ok(url) = obj.cast::<Self>() {
            return Ok(url.get().reference.clone());
        }
        match obj.cast::<PyString>() {
            Ok(s) => UrlReference::parse(&s.to_cow()?).map_err(op),
            Err(_) => Err(PyTypeError::new_err(format!(
                "Invalid type for url. Expected str or rqx.URL, got {}",
                obj.get_type().name()?
            ))),
        }
    }

    fn with_params(&self, params: QueryPairs) -> PyResult<Self> {
        Ok(Self::new(self.inner.with_params(params)?))
    }

    fn from_base_url(base_url: BaseUrl) -> Self {
        let url_reference = UrlReference::from_url(base_url.get_inner().clone());
        let client_url = RqxClientUrl::new(reference);
        Self::new(client_url)
    }
}

#[pymethods]
impl PyURL {
    // #[new]
    // #[pyo3(signature = (url=None, **kwargs))]
    // fn py_new(
    //     url: Option<&Bound<'_, PyAny>>,
    //     kwargs: Option<&Bound<'_, PyDict>>,
    // ) -> PyResult<Self> {
    //     let base = url.map(Self::extract_reference).transpose()?;
    //     match kwargs {
    //         None => Ok(Self::new(match base {
    //             Some(reference) => reference,
    //             None => UrlReference::parse("")?,
    //         })),
    //         Some(kwargs) => Ok(Self::new(UrlReference::compose(
    //             base.as_ref(),
    //             UrlComponents::extract(kwargs)?,
    //         )?)),
    //     }
    // }

    #[getter]
    fn scheme(&self) -> &str {
        self.inner.scheme()
    }

    #[getter]
    fn username(&self) -> &str {
        self.inner.username()
    }

    #[getter]
    fn password(&self) -> &str {
        self.inner.password()
    }

    #[getter]
    fn host(&self) -> String {
        self.inner.host()
    }

    #[getter]
    fn port(&self) -> Option<u16> {
        self.inner.port()
    }

    #[getter]
    fn path(&self) -> String {
        self.inner.path()
    }

    #[getter]
    fn query<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.query())
    }

    #[getter]
    fn params(&self) -> PyQueryParams {
        PyQueryParams::new(self.inner.params())
    }

    #[getter]
    fn raw_path<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.iner.raw_path())
    }

    #[getter]
    fn fragment(&self) -> &str {
        self.reference.fragment()
    }

    #[getter]
    fn is_absolute_url(&self) -> bool {
        self.reference.is_absolute()
    }

    #[getter]
    fn is_relative_url(&self) -> bool {
        !self.reference.is_absolute()
    }

    #[pyo3(signature = (**kwargs))]
    fn copy_with(&self, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let components = match kwargs {
            Some(kwargs) => UrlComponents::extract(kwargs)?,
            None => UrlComponents::default(),
        };
        Ok(Self::new(UrlReference::compose(
            Some(&self.reference),
            components,
        )?))
    }

    #[pyo3(signature = (key, value=None))]
    fn copy_set_param(&self, key: &str, value: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        self.inner.copy_set_param()
    }

    #[pyo3(signature = (key, value=None))]
    fn copy_add_param(&self, key: &str, value: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        self.with_params(
            self.reference
                .params()
                .add(key, QueryPairs::scalar_or_empty(value)?),
        )
    }

    fn copy_remove_param(&self, key: &str) -> PyResult<Self> {
        self.with_params(self.reference.params().remove(key))
    }

    #[pyo3(signature = (params=None))]
    fn copy_merge_params(&self, params: Option<QueryPairs>) -> PyResult<Self> {
        match params {
            Some(params) => self.with_params(self.reference.params().merge(&params)),
            None => Ok(Self::new(self.reference.clone())),
        }
    }

    /// A `str` joins as written: parsing it first would resolve its dot
    /// segments against nothing and lose them.
    fn join(&self, url: &Bound<'_, PyAny>) -> PyResult<Self> {
        match url.cast::<PyString>() {
            Ok(text) => match text.to_str() {
                Ok(t) => self.inner.join(&t),
                Err(_) => todo!(),
            },
            Err(_) => Self::extract_reference(url)?.to_string(),
        }
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!("URL('{}')", self.inner.masked())
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<PyString>() {
            Ok(url) => match url.to_str() {
                Ok(u) => self.inner.equals(u),
                // prob need a proper error raised instead of just going false.
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    fn __hash__(&self) -> u64 {
        self.inner.hash()
    }
}
