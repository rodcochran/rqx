use http::header::{HeaderMap, HeaderName, HeaderValue};
use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyList;
use std::collections::HashMap;
use std::str::FromStr;

use rqx_core::headers::Headers;

/// Case-insensitive header dict.
///
/// Backed by `http::HeaderMap`, the canonical Rust structure for HTTP headers.
/// Gets case-insensitive lookup, multi-value support, and battle-tested
/// semantics for free.
#[pyclass]
pub struct PyHeaders {
    pub(crate) inner: Headers,
}

#[pymethods]
impl PyHeaders {
    #[new]
    #[pyo3(signature = (init=None))]
    fn __new__(init: Option<HashMap<String, String>>) -> PyResult<Self> {
        Ok(
            Self {
                inner: Headers::new(init)?,
            },
        )
    }

    fn __getitem__(&self, key: &str) -> PyResult<String> {
        let name = HeaderName::from_str(key).map_err(|_| PyKeyError::new_err(key.to_string()))?;
        let values: Vec<&str> = self
            .inner
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
        self.inner.set_item(
            key, value,
        )
    }

    fn __delitem__(&mut self, key: &str) -> PyResult<()> {
        self.inner.delete_item(key)
    }

    fn __contains__(&self, key: &str) -> bool {
        self.inner.contains(key)
    }

    fn __iter__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let keys: Vec<String> = slf
            .inner
            .inner
            .keys()
            .map(|k| k.as_str().to_string())
            .collect();
        let list = PyList::new(
            py, &keys,
        )?;
        Ok(list.try_iter()?.into())
    }

    fn __len__(&self) -> usize {
        self.inner.length()
    }

    fn __repr__(&self) -> String {
        format!(
            "Headers({:?})",
            self.inner.inner
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        if let Ok(other_headers) = other.cast::<PyHeaders>() {
            return Ok(self.inner.inner == other_headers.borrow().inner.inner);
        }
        if let Ok(other_map) = other.extract::<HashMap<String, String>>() {
            let mut other_inner = HeaderMap::try_with_capacity(other_map.len()).unwrap_or_default();
            for (k, v) in other_map {
                let name = match HeaderName::from_str(&k) {
                    Ok(n) => n,
                    Err(_) => return Ok(false),
                };
                let value = match HeaderValue::from_str(&v) {
                    Ok(v) => v,
                    Err(_) => return Ok(false),
                };
                // A mapping too large to hold can't equal this one.
                if other_inner
                    .try_insert(
                        name, value,
                    )
                    .is_err()
                {
                    return Ok(false);
                }
            }
            return Ok(self.inner.inner == other_inner);
        }
        Ok(false)
    }

    #[pyo3(signature = (key, default=None))]
    fn get(&self, key: &str, default: Option<String>) -> Option<String> {
        self.__getitem__(key).ok().or(default)
    }

    fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }

    fn values(&self) -> Vec<String> {
        self.inner.values()
    }

    fn items(
        &self,
    ) -> Vec<(
        String,
        String,
    )> {
        // Includes duplicates (Set-Cookie, etc.) — same as iterating HeaderMap directly.
        self.inner.items()
    }
}
