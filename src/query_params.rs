//! `rqx.QueryParams`: an immutable multi-dict, httpx's semantics
//! (https://github.com/rodcochran/rqx/issues/59).

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyInt, PyIterator, PyList, PyString, PyTuple};
use url::form_urlencoded;

#[derive(Clone, Default)]
pub struct QueryPairs(Vec<(String, String)>);

impl QueryPairs {
    pub fn pairs(&self) -> &[(String, String)] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn parse(query: &str) -> Self {
        let mut pairs = Self::default();
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            pairs.push(key.into_owned(), value.into_owned());
        }
        pairs
    }

    /// Values sit under the first appearance of their key, as they would in
    /// the `dict[str, list[str]]` httpx keeps.
    fn push(&mut self, key: String, value: String) {
        match self.0.iter().rposition(|(k, _)| *k == key) {
            Some(last) => self.0.insert(last + 1, (key, value)),
            None => self.0.push((key, value)),
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_list(&self, key: &str) -> Vec<&str> {
        self.0
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = Vec::new();
        for (key, _) in &self.0 {
            if !keys.contains(&key.as_str()) {
                keys.push(key);
            }
        }
        keys
    }

    pub fn first_items(&self) -> Vec<(&str, &str)> {
        self.keys()
            .into_iter()
            .map(|key| (key, self.get(key).unwrap_or_default()))
            .collect()
    }

    pub fn set(&self, key: &str, value: String) -> Self {
        let mut out = Vec::with_capacity(self.0.len() + 1);
        let mut replaced = false;
        for (k, v) in &self.0 {
            if k != key {
                out.push((k.clone(), v.clone()));
            } else if !replaced {
                out.push((key.to_owned(), value.clone()));
                replaced = true;
            }
        }
        if !replaced {
            out.push((key.to_owned(), value));
        }
        Self(out)
    }

    pub fn add(&self, key: &str, value: String) -> Self {
        let mut out = self.clone();
        out.push(key.to_owned(), value);
        out
    }

    pub fn remove(&self, key: &str) -> Self {
        Self(self.0.iter().filter(|(k, _)| k != key).cloned().collect())
    }

    /// `other` wins for any key it carries, in place; its new keys go last.
    pub fn merge(&self, other: &Self) -> Self {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut taken: Vec<&str> = Vec::new();
        for (key, value) in &self.0 {
            if !other.contains(key) {
                out.push((key.clone(), value.clone()));
                continue;
            }
            if !taken.contains(&key.as_str()) {
                taken.push(key);
                out.extend(
                    other
                        .get_list(key)
                        .iter()
                        .map(|v| (key.clone(), (*v).to_owned())),
                );
            }
        }
        for (key, value) in &other.0 {
            if !taken.contains(&key.as_str()) {
                out.push((key.clone(), value.clone()));
            }
        }
        Self(out)
    }

    /// Equality ignores order, within a key as well as across keys, so the
    /// hash is built from the same thing.
    fn sorted(&self) -> Vec<&(String, String)> {
        let mut pairs: Vec<&(String, String)> = self.0.iter().collect();
        pairs.sort();
        pairs
    }

    fn from_py(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(params) = obj.cast::<PyQueryParams>() {
            return Ok(params.get().pairs.clone());
        }
        if let Ok(s) = obj.cast::<PyString>() {
            return Ok(Self::parse(&s.to_cow()?));
        }
        if let Ok(b) = obj.cast::<PyBytes>() {
            return Ok(Self::parse(&String::from_utf8_lossy(b.as_bytes())));
        }
        if let Ok(dict) = obj.cast::<PyDict>() {
            return Self::from_items(dict.iter());
        }
        if let Ok(items) = obj.getattr("items") {
            let items = items.call0()?;
            return Self::from_items(
                items
                    .try_iter()?
                    .map(|item| item?.extract::<(Bound<'_, PyAny>, Bound<'_, PyAny>)>())
                    .collect::<PyResult<Vec<_>>>()?,
            );
        }
        if let Ok(iter) = obj.try_iter() {
            return Self::from_items(
                iter.map(|item| item?.extract::<(Bound<'_, PyAny>, Bound<'_, PyAny>)>())
                    .collect::<PyResult<Vec<_>>>()?,
            );
        }
        Err(PyTypeError::new_err(format!(
            "params must be a mapping, a sequence of pairs, str, or bytes, got {}",
            obj.get_type().name()?
        )))
    }

    fn from_items<'py>(
        items: impl IntoIterator<Item = (Bound<'py, PyAny>, Bound<'py, PyAny>)>,
    ) -> PyResult<Self> {
        let mut pairs = Self::default();
        for (key, value) in items {
            let key = Self::key(&key)?;
            for value in Self::values(&value)? {
                pairs.push(key.clone(), value);
            }
        }
        Ok(pairs)
    }

    fn key(key: &Bound<'_, PyAny>) -> PyResult<String> {
        match key.cast::<PyString>() {
            Ok(s) => Ok(s.to_cow()?.into_owned()),
            Err(_) => Err(PyTypeError::new_err(format!(
                "params keys must be str, got {}",
                key.get_type().name()?
            ))),
        }
    }

    fn values(value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
        if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
            return value
                .try_iter()?
                .map(|item| Self::scalar(&item?))
                .collect::<PyResult<Vec<_>>>();
        }
        Ok(vec![Self::scalar(value)?])
    }

    /// httpx's `primitive_value_to_str`: `None` is an empty value (the key
    /// stays), bools are lowercase, numbers keep Python's `str()` form.
    pub fn scalar_or_empty(value: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
        match value {
            Some(value) => Self::scalar(value),
            None => Ok(String::new()),
        }
    }

    pub fn scalar(value: &Bound<'_, PyAny>) -> PyResult<String> {
        if value.is_none() {
            return Ok(String::new());
        }
        // bool before int: Python's bool is an int subclass.
        if let Ok(b) = value.cast::<PyBool>() {
            return Ok(if b.is_true() { "true" } else { "false" }.to_owned());
        }
        if let Ok(s) = value.cast::<PyString>() {
            return Ok(s.to_cow()?.into_owned());
        }
        if value.is_instance_of::<PyInt>() {
            if let Ok(n) = value.extract::<i64>() {
                return Ok(n.to_string());
            }
            return Ok(value.str()?.to_cow()?.into_owned());
        }
        if value.is_instance_of::<PyFloat>() {
            // Python's str(): `1e+16`, `nan`, `inf`. Rust's Display differs.
            return Ok(value.str()?.to_cow()?.into_owned());
        }
        Err(PyTypeError::new_err(format!(
            "params values must be str, int, float, bool, or None, got {}",
            value.get_type().name()?
        )))
    }
}

impl fmt::Display for QueryPairs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut encoded = form_urlencoded::Serializer::new(String::new());
        for (key, value) in &self.0 {
            encoded.append_pair(key, value);
        }
        f.write_str(&encoded.finish())
    }
}

impl PartialEq for QueryPairs {
    fn eq(&self, other: &Self) -> bool {
        self.sorted() == other.sorted()
    }
}

impl<'py> FromPyObject<'_, 'py> for QueryPairs {
    type Error = PyErr;

    fn extract(obj: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        Self::from_py(&obj.to_owned())
    }
}

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
        let pairs = match (params, kwargs) {
            (Some(pairs), _) => pairs,
            (None, Some(pairs)) => pairs,
            (None, None) => QueryPairs::default(),
        };
        Self::new(pairs)
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
