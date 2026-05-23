use http::header::{HeaderMap, HeaderName, HeaderValue};
use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyList;
use std::collections::HashMap;
use std::str::FromStr;

/// Case-insensitive header dict.
///
/// Backed by `http::HeaderMap`, the canonical Rust structure for HTTP headers.
/// Gets case-insensitive lookup, multi-value support, and battle-tested
/// semantics for free.
#[pyclass]
pub struct PyHeaders {
    inner: HeaderMap,
}

#[pymethods]
impl PyHeaders {
    #[new]
    #[pyo3(signature = (init=None))]
    fn __new__(init: Option<HashMap<String, String>>) -> PyResult<Self> {
        let mut inner = HeaderMap::new();
        if let Some(map) = init {
            for (k, v) in map {
                let name = HeaderName::from_str(&k).map_err(|e| {
                    PyValueError::new_err(format!("invalid header name {k:?}: {e}"))
                })?;
                let value = HeaderValue::from_str(&v).map_err(|e| {
                    PyValueError::new_err(format!("invalid header value {v:?}: {e}"))
                })?;
                inner.insert(name, value);
            }
        }
        Ok(Self { inner })
    }

    fn __getitem__(&self, key: &str) -> PyResult<String> {
        let name = HeaderName::from_str(key).map_err(|_| PyKeyError::new_err(key.to_string()))?;
        let values: Vec<&str> = self
            .inner
            .get_all(&name)
            .iter()
            .map(|v| v.to_str().unwrap_or(""))
            .collect();
        if values.is_empty() {
            return Err(PyKeyError::new_err(key.to_string()));
        }
        Ok(values.join(", "))
    }

    fn __setitem__(&mut self, key: &str, value: String) -> PyResult<()> {
        let name = HeaderName::from_str(key)
            .map_err(|e| PyValueError::new_err(format!("invalid header name {key:?}: {e}")))?;
        let val = HeaderValue::from_str(&value)
            .map_err(|e| PyValueError::new_err(format!("invalid header value {value:?}: {e}")))?;
        self.inner.insert(name, val); // replaces existing entries with this name
        Ok(())
    }

    fn __delitem__(&mut self, key: &str) -> PyResult<()> {
        let name = HeaderName::from_str(key).map_err(|_| PyKeyError::new_err(key.to_string()))?;
        if self.inner.remove(&name).is_none() {
            return Err(PyKeyError::new_err(key.to_string()));
        }
        Ok(())
    }

    fn __contains__(&self, key: &str) -> bool {
        HeaderName::from_str(key)
            .map(|name| self.inner.contains_key(&name))
            .unwrap_or(false)
    }

    fn __iter__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let keys: Vec<String> = slf.inner.keys().map(|k| k.as_str().to_string()).collect();
        let list = PyList::new(py, &keys)?;
        Ok(list.try_iter()?.into())
    }

    fn __len__(&self) -> usize {
        self.inner.keys_len()
    }

    fn __repr__(&self) -> String {
        format!("Headers({:?})", self.inner)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        if let Ok(other_headers) = other.cast::<PyHeaders>() {
            return Ok(self.inner == other_headers.borrow().inner);
        }
        if let Ok(other_map) = other.extract::<HashMap<String, String>>() {
            let mut other_inner = HeaderMap::with_capacity(other_map.len());
            for (k, v) in other_map {
                let name = match HeaderName::from_str(&k) {
                    Ok(n) => n,
                    Err(_) => return Ok(false),
                };
                let value = match HeaderValue::from_str(&v) {
                    Ok(v) => v,
                    Err(_) => return Ok(false),
                };
                other_inner.insert(name, value);
            }
            return Ok(self.inner == other_inner);
        }
        Ok(false)
    }

    #[pyo3(signature = (key, default=None))]
    fn get(&self, key: &str, default: Option<String>) -> Option<String> {
        self.__getitem__(key).ok().or(default)
    }

    fn keys(&self) -> Vec<String> {
        self.inner.keys().map(|k| k.as_str().to_string()).collect()
    }

    fn values(&self) -> Vec<String> {
        self.inner
            .values()
            .map(|v| v.to_str().unwrap_or("").to_string())
            .collect()
    }

    fn items(&self) -> Vec<(String, String)> {
        // Includes duplicates (Set-Cookie, etc.) — same as iterating HeaderMap directly.
        self.inner
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect()
    }
}

// Rust-only helpers (not exposed to Python).
impl PyHeaders {
    /// Build from `Vec<(name, value)>` — used by response construction where
    /// the data came from reqwest's iteration.
    pub fn from_pairs(items: Vec<(String, String)>) -> Self {
        let mut inner = HeaderMap::with_capacity(items.len());
        for (k, v) in items {
            // Skip malformed names/values defensively. reqwest's HeaderMap
            // shouldn't ever produce them, but we don't want to panic if
            // something pathological slips through.
            if let (Ok(name), Ok(value)) = (HeaderName::from_str(&k), HeaderValue::from_str(&v)) {
                inner.append(name, value); // append preserves multi-values
            }
        }
        Self { inner }
    }

    /// Return the first value matching `key` (case-insensitive). Used by
    /// Rust-side code that just wants a single header for internal logic.
    pub fn get_first(&self, key: &str) -> Option<&str> {
        HeaderName::from_str(key)
            .ok()
            .and_then(|name| self.inner.get(&name))
            .and_then(|v| v.to_str().ok())
    }

    pub fn from_header_map(header_map: HeaderMap) -> Self {
        Self { inner: header_map }
    }
}
