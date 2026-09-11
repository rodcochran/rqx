use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyString};

/// The `params=` kwarg: a str-keyed mapping whose values are str, int, float,
/// bool, or None. Coerced the way httpx does it (`true`/`false`, None drops
/// the key, floats keep Python's `str()` form). Order is the wire order
/// (https://github.com/rodcochran/rqx/issues/115).
pub struct QueryParams(Vec<(String, String)>);

impl QueryParams {
    pub fn pairs(&self) -> &[(String, String)] {
        &self.0
    }

    fn from_items<'py>(
        items: impl Iterator<Item = PyResult<(Bound<'py, PyAny>, Bound<'py, PyAny>)>>,
    ) -> PyResult<Self> {
        let mut pairs = Vec::new();
        for item in items {
            let (key, value) = item?;
            if let Some(value) = Self::value(&value)? {
                pairs.push((Self::key(&key)?, value));
            }
        }
        Ok(Self(pairs))
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

    /// `Ok(None)` means the key is dropped. bool is checked before int
    /// because Python's bool is an int subclass.
    fn value(value: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
        if value.is_none() {
            return Ok(None);
        }
        if let Ok(b) = value.cast::<PyBool>() {
            return Ok(Some(if b.is_true() { "true" } else { "false" }.to_owned()));
        }
        if let Ok(s) = value.cast::<PyString>() {
            return Ok(Some(s.to_cow()?.into_owned()));
        }
        if value.is_instance_of::<PyInt>() {
            if let Ok(n) = value.extract::<i64>() {
                return Ok(Some(n.to_string()));
            }
            return Ok(Some(value.str()?.to_cow()?.into_owned()));
        }
        if value.is_instance_of::<PyFloat>() {
            // Python's str(): `1e+16`, `nan`, `inf`. Rust's Display differs.
            return Ok(Some(value.str()?.to_cow()?.into_owned()));
        }
        Err(PyTypeError::new_err(format!(
            "params values must be str, int, float, bool, or None, got {}",
            value.get_type().name()?
        )))
    }
}

impl<'py> FromPyObject<'_, 'py> for QueryParams {
    type Error = PyErr;

    fn extract(obj: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        if let Ok(dict) = obj.cast::<PyDict>() {
            return Self::from_items(dict.iter().map(Ok));
        }
        let Ok(items) = obj.getattr("items") else {
            return Err(PyTypeError::new_err(format!(
                "params must be a mapping, got {}",
                obj.get_type().name()?
            )));
        };
        Self::from_items(
            items
                .call0()?
                .try_iter()?
                .map(|item| item?.extract::<(Bound<'py, PyAny>, Bound<'py, PyAny>)>()),
        )
    }
}
