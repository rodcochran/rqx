use pyo3::exceptions::PyTypeError;
use pyo3::prelude::{PyAny, PyResult};
use pyo3::types::{PyAnyMethods, PyBool, PyFloat, PyInt, PyString};
use pyo3::{Borrowed, Bound, FromPyObject, PyErr};
use std::collections::HashMap;

#[derive(Debug)]
pub struct QueryParams(pub(crate) HashMap<String, String>);

impl<'py> FromPyObject<'_, 'py> for QueryParams {
    type Error = PyErr;

    fn extract(obj: Borrowed<'_, 'py, PyAny>) -> Result<Self, Self::Error> {
        let items = obj
            .call_method0("items")
            .map_err(|_| PyTypeError::new_err("params must be a mapping"))?;
        let mut params = HashMap::new();

        for item in items.try_iter()? {
            let (key, value): (String, Bound<'py, PyAny>) = item?.extract()?;
            if let Some(value) = query_param_to_string(&value)? {
                params.insert(key, value);
            }
        }

        Ok(Self(params))
    }
}

fn query_param_to_string(value: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    if value.is_none() {
        return Ok(None);
    }

    if value.is_instance_of::<PyBool>() {
        return Ok(Some(
            if value.extract::<bool>()? {
                "true"
            } else {
                "false"
            }
            .to_owned(),
        ));
    }

    if value.is_instance_of::<PyString>()
        || value.is_instance_of::<PyInt>()
        || value.is_instance_of::<PyFloat>()
    {
        return value.str()?.extract::<String>().map(Some);
    }

    Err(PyTypeError::new_err(
        "params values must be str, int, float, bool, or None",
    ))
}
