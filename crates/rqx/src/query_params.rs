//! `rqx.QueryParams`: an immutable multi-dict, httpx's semantics
//! (https://github.com/rodcochran/rqx/issues/59).

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyInt, PyIterator, PyList, PyString, PyTuple};
use url::form_urlencoded;

use rqx_core::query_params::QueryPairs;

#[pyclass(name = "QueryParams", module = "rqx", frozen)]
pub struct PyQueryParams {
    pairs: QueryPairs,
}

impl PyQueryParams {
    pub fn new(pairs: QueryPairs) -> Self {
        Self { pairs }
    }

    pub fn pairs(&self) -> &QueryPairs {
        &self.pairs
    }

    fn immutable(action: &str) -> PyErr {
        PyRuntimeError::new_err(format!(
            "QueryParams are immutable since 0.18.0. Use `q = q.{action}` to create an updated copy."
        ))
    }
}

#[pymethods]
impl PyQueryParams {
    #[new]
    #[pyo3(signature = (params=None, **kwargs))]
    fn py_new(params: Option<QueryPairs>, kwargs: Option<QueryPairs>) -> Self {
        Self::new(params.or(kwargs).unwrap_or_default())
    }

    #[pyo3(signature = (key, default=None))]
    fn get(&self, py: Python<'_>, key: &str, default: Option<Py<PyAny>>) -> Option<Py<PyAny>> {
        match self.pairs.get(key) {
            Some(value) => Some(PyString::new(py, value).into_any().unbind()),
            None => default,
        }
    }

    fn get_list(&self, key: &str) -> Vec<&str> {
        self.pairs.get_list(key)
    }

    fn keys(&self) -> Vec<&str> {
        self.pairs.keys()
    }

    fn values(&self) -> Vec<&str> {
        self.pairs
            .first_items()
            .into_iter()
            .map(|(_, value)| value)
            .collect()
    }

    fn items(&self) -> Vec<(&str, &str)> {
        self.pairs.first_items()
    }

    fn multi_items(&self) -> Vec<(&str, &str)> {
        self.pairs
            .pairs()
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect()
    }

    fn set(&self, key: &str, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self::new(self.pairs.set(key, QueryPairs::scalar(value)?)))
    }

    fn add(&self, key: &str, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self::new(self.pairs.add(key, QueryPairs::scalar(value)?)))
    }

    fn remove(&self, key: &str) -> Self {
        Self::new(self.pairs.remove(key))
    }

    #[pyo3(signature = (params=None))]
    fn merge(&self, params: Option<QueryPairs>) -> Self {
        match params {
            Some(other) => Self::new(self.pairs.merge(&other)),
            None => Self::new(self.pairs.clone()),
        }
    }

    #[pyo3(signature = (*_args, **_kwargs))]
    fn update(
        &self,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        Err(Self::immutable("merge(...)"))
    }

    fn __setitem__(&self, _key: &str, _value: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(Self::immutable("set(key, value)"))
    }

    fn __getitem__(&self, key: &str) -> PyResult<&str> {
        self.pairs
            .get(key)
            .ok_or_else(|| PyKeyError::new_err(key.to_owned()))
    }

    fn __contains__(&self, key: &str) -> bool {
        self.pairs.contains(key)
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        PyList::new(py, self.pairs.keys())?.try_iter()
    }

    fn __len__(&self) -> usize {
        self.pairs.keys().len()
    }

    fn __bool__(&self) -> bool {
        !self.pairs.is_empty()
    }

    fn __str__(&self) -> String {
        self.pairs.to_string()
    }

    fn __repr__(&self) -> String {
        format!("QueryParams('{}')", self.pairs)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<Self>() {
            Ok(other) => self.pairs == other.get().pairs,
            Err(_) => false,
        }
    }

    /// Hashed from what `__eq__` compares. httpx hashes `str(self)`, which
    /// breaks that contract for two params it calls equal.
    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.pairs.sorted().hash(&mut hasher);
        hasher.finish()
    }
}
