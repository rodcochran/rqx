use pyo3::prelude::*;
use pyo3::types::{PyIterator, PyList};
use std::collections::HashMap;

use rqx_core::headers::Headers;

use crate::exceptions::PyRqxError;
use crate::request_headers::RequestHeaders;

/// Case-insensitive header dict.
#[pyclass]
pub struct PyHeaders {
    pub(crate) inner: Headers,
}

impl PyHeaders {
    pub fn new(inner: Headers) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyHeaders {
    #[new]
    #[pyo3(signature = (init=None))]
    fn __new__(init: Option<HashMap<String, String>>) -> Result<Self, PyRqxError> {
        Ok(Self::new(Headers::new(init)?))
    }

    fn __getitem__(&self, key: &str) -> Result<String, PyRqxError> {
        Ok(self.inner.get_joined_values_for_key(key)?)
    }

    fn __setitem__(&mut self, key: &str, value: String) -> Result<(), PyRqxError> {
        Ok(self.inner.set_item(key, value)?)
    }

    fn __delitem__(&mut self, key: &str) -> Result<(), PyRqxError> {
        Ok(self.inner.delete_item(key)?)
    }

    fn __contains__(&self, key: &str) -> bool {
        self.inner.contains(key)
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        PyList::new(py, self.inner.keys())?.try_iter()
    }

    fn __len__(&self) -> usize {
        self.inner.length()
    }

    fn __repr__(&self) -> String {
        format!("Headers({:?})", self.inner)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.extract::<RequestHeaders>() {
            Ok(other) => self.inner == other.inner,
            Err(_) => false,
        }
    }

    #[pyo3(signature = (key, default=None))]
    fn get(&self, key: &str, default: Option<String>) -> Option<String> {
        self.inner.get_joined_values_for_key(key).ok().or(default)
    }

    fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }

    fn values(&self) -> Vec<String> {
        self.inner.values()
    }

    fn items(&self) -> Vec<(String, String)> {
        self.inner.items()
    }
}
