use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytes::Bytes;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyString};

use rqx_core::query_params::QueryPairs;
use rqx_core::url::request_url::BaseUrl;
use rqx_core::url::{
    client_url::RqxClientUrl, components::UrlComponentValue, reference::UrlReference,
};

use crate::exceptions::PyRqxError;
use crate::query_params::{PyQueryParams, PyScalarValue};

struct UrlKwargs {
    inner: HashMap<String, Option<UrlComponentValue>>,
}

impl UrlKwargs {
    fn extract(kwargs: Option<&Bound<'_, PyDict>>) -> Result<Self, PyRqxError> {
        let mut inner = HashMap::new();
        if let Some(kwargs) = kwargs {
            for (key, value) in kwargs.iter() {
                inner.insert(key.extract::<String>()?, Self::value(&value)?);
            }
        }
        Ok(Self { inner })
    }

    fn value(value: &Bound<'_, PyAny>) -> Result<Option<UrlComponentValue>, PyRqxError> {
        if value.is_none() {
            return Ok(None);
        }
        if let Ok(text) = value.cast::<PyString>() {
            return Ok(Some(UrlComponentValue::String(text.to_cow()?.into_owned())));
        }
        if let Ok(raw) = value.cast::<PyBytes>() {
            return Ok(Some(UrlComponentValue::Bytes(Bytes::copy_from_slice(
                raw.as_bytes(),
            ))));
        }
        if let Ok(params) = value.cast::<PyQueryParams>() {
            return Ok(Some(UrlComponentValue::QueryPairs(
                params.get().pairs().clone(),
            )));
        }
        if let Ok(n) = value.extract::<u16>() {
            return Ok(Some(UrlComponentValue::Int(n)));
        }
        Err(PyTypeError::new_err(format!(
            "URL components must be str, bytes, int, QueryParams or None, got {}",
            value.get_type().name()?
        ))
        .into())
    }
}

#[pyclass(name = "URL", module = "rqx", frozen, skip_from_py_object)]
pub struct PyURL {
    pub(crate) inner: RqxClientUrl,
}

impl PyURL {
    pub fn new(url: RqxClientUrl) -> Self {
        Self { inner: url }
    }

    pub(crate) fn from_base_url(base_url: &BaseUrl) -> Self {
        Self::new(RqxClientUrl::new(UrlReference::from_url(
            base_url.get_inner(),
        )))
    }
}

#[pymethods]
impl PyURL {
    #[new]
    #[pyo3(signature = (url=None, **kwargs))]
    fn py_new(url: Option<PyURL>, kwargs: Option<&Bound<'_, PyDict>>) -> Result<Self, PyRqxError> {
        let base = match url {
            Some(url) => url,
            None => Self::new(RqxClientUrl::parse("")?),
        };
        base.copy_with(kwargs)
    }

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
        PyBytes::new(py, self.inner.query().as_bytes())
    }

    #[getter]
    fn params(&self) -> PyQueryParams {
        PyQueryParams::new(self.inner.params())
    }

    #[getter]
    fn raw_path<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.raw_path().as_bytes())
    }

    #[getter]
    fn fragment(&self) -> &str {
        self.inner.fragment()
    }

    #[getter]
    fn is_absolute_url(&self) -> bool {
        self.inner.is_absolute_url()
    }

    #[getter]
    fn is_relative_url(&self) -> bool {
        self.inner.is_relative_url()
    }

    #[pyo3(signature = (**kwargs))]
    fn copy_with(&self, kwargs: Option<&Bound<'_, PyDict>>) -> Result<Self, PyRqxError> {
        Ok(Self::new(
            self.inner.copy_with(UrlKwargs::extract(kwargs)?.inner)?,
        ))
    }

    #[pyo3(signature = (key, value=None))]
    fn copy_set_param(&self, key: &str, value: Option<PyScalarValue>) -> Result<Self, PyRqxError> {
        Ok(Self::new(
            self.inner.copy_set_param(key, value.map(|v| v.inner))?,
        ))
    }

    #[pyo3(signature = (key, value=None))]
    fn copy_add_param(&self, key: &str, value: Option<PyScalarValue>) -> Result<Self, PyRqxError> {
        Ok(Self::new(
            self.inner.copy_add_param(key, value.map(|v| v.inner))?,
        ))
    }

    fn copy_remove_param(&self, key: &str) -> Result<Self, PyRqxError> {
        Ok(Self::new(self.inner.copy_remove_param(key)?))
    }

    #[pyo3(signature = (params=None))]
    fn copy_merge_params(&self, params: Option<QueryPairs>) -> Result<Self, PyRqxError> {
        Ok(Self::new(self.inner.copy_merge_params(params)?))
    }

    /// A `str` joins as written: parsing it first would resolve its dot
    /// segments against nothing and lose them.
    fn join(&self, url: &Bound<'_, PyAny>) -> Result<Self, PyRqxError> {
        let other = match url.cast::<PyString>() {
            Ok(text) => text.to_cow()?.into_owned(),
            Err(_) => url.extract::<Self>()?.inner.to_string(),
        };
        Ok(Self::new(self.inner.join(&other)?))
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!("URL('{}')", self.inner.masked())
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.extract::<Self>() {
            Ok(other) => self.inner == other.inner,
            Err(_) => false,
        }
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }
}

/// A `str` or an `rqx.URL`, either way one `PyURL`.
impl<'py> FromPyObject<'_, 'py> for PyURL {
    type Error = PyRqxError;

    fn extract(obj: Borrowed<'_, 'py, PyAny>) -> Result<Self, Self::Error> {
        if let Ok(url) = obj.cast::<Self>() {
            return Ok(Self::new(url.get().inner.clone()));
        }
        match obj.cast::<PyString>() {
            Ok(s) => Ok(Self::new(RqxClientUrl::parse(&s.to_cow()?)?)),
            Err(_) => Err(PyTypeError::new_err(format!(
                "Invalid type for url. Expected str or rqx.URL, got {}",
                obj.get_type().name()?
            ))
            .into()),
        }
    }
}
