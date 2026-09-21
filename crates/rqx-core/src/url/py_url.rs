use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyString};

use super::reference::{UrlComponents, UrlReference};
use crate::query_params::{PyQueryParams, QueryPairs};

#[pyclass(name = "URL", module = "rqx", frozen)]
pub struct PyURL {
    reference: UrlReference,
}

impl PyURL {
    pub fn new(reference: UrlReference) -> Self {
        Self { reference }
    }

    // This needs to move to the rqx crate...
    pub fn extract_reference(obj: &Bound<'_, PyAny>) -> PyResult<UrlReference> {
        if let Ok(url) = obj.cast::<Self>() {
            return Ok(url.get().reference.clone());
        }
        match obj.cast::<PyString>() {
            Ok(s) => UrlReference::parse(&s.to_cow()?),
            Err(_) => Err(PyTypeError::new_err(format!(
                "Invalid type for url. Expected str or rqx.URL, got {}",
                obj.get_type().name()?
            ))),
        }
    }

    fn with_params(&self, params: QueryPairs) -> PyResult<Self> {
        Ok(Self::new(self.reference.with_params(&params)?))
    }
}

#[pymethods]
impl PyURL {
    #[new]
    #[pyo3(signature = (url=None, **kwargs))]
    fn py_new(
        url: Option<&Bound<'_, PyAny>>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let base = url.map(Self::extract_reference).transpose()?;
        match kwargs {
            None => Ok(Self::new(match base {
                Some(reference) => reference,
                None => UrlReference::parse("")?,
            })),
            Some(kwargs) => Ok(Self::new(UrlReference::compose(
                base.as_ref(),
                UrlComponents::extract(kwargs)?,
            )?)),
        }
    }

    #[getter]
    fn scheme(&self) -> &str {
        self.reference.scheme()
    }

    #[getter]
    fn username(&self) -> &str {
        self.reference.username()
    }

    #[getter]
    fn password(&self) -> &str {
        self.reference.password()
    }

    #[getter]
    fn host(&self) -> String {
        self.reference.host().into_owned()
    }

    #[getter]
    fn port(&self) -> Option<u16> {
        self.reference.port()
    }

    #[getter]
    fn path(&self) -> String {
        self.reference.path().into_owned()
    }

    #[getter]
    fn query<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.reference.query().as_bytes())
    }

    #[getter]
    fn params(&self) -> PyQueryParams {
        PyQueryParams::new(self.reference.params())
    }

    #[getter]
    fn raw_path<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.reference.raw_path().as_bytes())
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
        self.with_params(
            self.reference
                .params()
                .set(key, QueryPairs::scalar_or_empty(value)?),
        )
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
        let other = match url.cast::<PyString>() {
            Ok(text) => text.to_cow()?.into_owned(),
            Err(_) => Self::extract_reference(url)?.to_string(),
        };
        Ok(Self::new(self.reference.join(&other)?))
    }

    fn __str__(&self) -> String {
        self.reference.to_string()
    }

    fn __repr__(&self) -> String {
        format!("URL('{}')", self.reference.masked())
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match Self::extract_reference(other) {
            Ok(other) => self.reference == other,
            Err(_) => false,
        }
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.reference.to_string().hash(&mut hasher);
        hasher.finish()
    }
}
