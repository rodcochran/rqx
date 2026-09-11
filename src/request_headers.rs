use http::header::{HeaderMap, HeaderName, HeaderValue};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedStr;
use pyo3::types::{PyDict, PyString};

use crate::headers::PyHeaders;

/// The `headers=` kwarg, built straight into a `HeaderMap` at the boundary:
/// no intermediate map, one validation pass, and a `ValueError` instead of
/// a panic on a bad name or value (https://github.com/rodcochran/rqx/issues/117).
/// Case-variant keys become separate header lines, like httpx.
pub struct RequestHeaders(HeaderMap);

impl RequestHeaders {
    pub fn into_map(self) -> HeaderMap {
        self.0
    }

    fn from_items<'py>(
        len: usize,
        items: impl Iterator<Item = PyResult<(Bound<'py, PyAny>, Bound<'py, PyAny>)>>,
    ) -> PyResult<Self> {
        let mut map = HeaderMap::with_capacity(len);
        for item in items {
            let (name, value) = item?;
            map.append(Self::name(&name)?, Self::value(&value)?);
        }
        Ok(Self(map))
    }

    /// Borrowed view of a `str`; anything else is a TypeError naming the role.
    fn text<'py>(obj: &Bound<'py, PyAny>, role: &str) -> PyResult<PyBackedStr> {
        match obj.cast::<PyString>() {
            Ok(s) => PyBackedStr::try_from(s.clone()),
            Err(_) => Err(PyTypeError::new_err(format!(
                "header {role}s must be str, got {}",
                obj.get_type().name()?
            ))),
        }
    }

    fn name(obj: &Bound<'_, PyAny>) -> PyResult<HeaderName> {
        let name = Self::text(obj, "name")?;
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| PyValueError::new_err(format!("invalid header name {:?}: {e}", &*name)))
    }

    fn value(obj: &Bound<'_, PyAny>) -> PyResult<HeaderValue> {
        let value = Self::text(obj, "value")?;
        HeaderValue::from_str(&value)
            .map_err(|e| PyValueError::new_err(format!("invalid header value {:?}: {e}", &*value)))
    }
}

impl<'py> FromPyObject<'_, 'py> for RequestHeaders {
    type Error = PyErr;

    fn extract(obj: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        if let Ok(headers) = obj.cast::<PyHeaders>() {
            return Ok(Self(headers.borrow().inner.clone()));
        }
        if let Ok(dict) = obj.cast::<PyDict>() {
            return Self::from_items(dict.len(), dict.iter().map(Ok));
        }
        let Ok(items) = obj.getattr("items") else {
            return Err(PyTypeError::new_err(format!(
                "headers must be a mapping or rqx.Headers, got {}",
                obj.get_type().name()?
            )));
        };
        Self::from_items(
            obj.len().unwrap_or(0),
            items
                .call0()?
                .try_iter()?
                .map(|item| item?.extract::<(Bound<'py, PyAny>, Bound<'py, PyAny>)>()),
        )
    }
}
