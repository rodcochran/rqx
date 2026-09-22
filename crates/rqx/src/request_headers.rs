use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use rqx_core::headers::Headers;

use crate::exceptions::PyRqxError;
use crate::headers::PyHeaders;

/// The `headers=` kwarg: an `rqx.Headers` or any mapping of `str` to `str`.
pub struct RequestHeaders {
    pub(crate) inner: Headers,
}

impl RequestHeaders {
    fn from_items<'py>(
        items: impl Iterator<Item = PyResult<(Bound<'py, PyAny>, Bound<'py, PyAny>)>>,
    ) -> Result<Self, PyRqxError> {
        let mut pairs = Vec::new();
        for item in items {
            let (name, value) = item?;
            pairs.push((Self::text(&name, "name")?, Self::text(&value, "value")?));
        }
        Ok(Self {
            inner: Headers::try_from_pairs(pairs)?,
        })
    }

    fn text(obj: &Bound<'_, PyAny>, role: &str) -> Result<String, PyRqxError> {
        match obj.cast::<PyString>() {
            Ok(s) => Ok(s.to_cow()?.into_owned()),
            Err(_) => Err(PyTypeError::new_err(format!(
                "header {role}s must be str, got {}",
                obj.get_type().name()?
            ))
            .into()),
        }
    }
}

impl<'py> FromPyObject<'_, 'py> for RequestHeaders {
    type Error = PyRqxError;

    fn extract(obj: Borrowed<'_, 'py, PyAny>) -> Result<Self, Self::Error> {
        if let Ok(headers) = obj.cast::<PyHeaders>() {
            return Ok(Self {
                inner: headers.borrow().inner.clone(),
            });
        }
        if let Ok(dict) = obj.cast::<PyDict>() {
            return Self::from_items(dict.iter().map(Ok));
        }
        let Ok(items) = obj.getattr("items") else {
            return Err(PyTypeError::new_err(format!(
                "headers must be a mapping or rqx.Headers, got {}",
                obj.get_type().name()?
            ))
            .into());
        };
        Self::from_items(
            items
                .call0()?
                .try_iter()?
                .map(|item| item?.extract::<(Bound<'py, PyAny>, Bound<'py, PyAny>)>()),
        )
    }
}
