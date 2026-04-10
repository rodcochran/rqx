use pyo3::conversion::{IntoPyObject, IntoPyObjectExt};
use pyo3::exceptions::{PyValueError};
use pyo3::prelude::{Py, PyAny, PyResult, Python};
use pyo3::types::{PyAnyMethods, PyBool, PyDict, PyDictMethods, PyFloat, PyInt, PyList, PyString};
use pyo3::Bound;

pub fn value_to_py(py: Python<'_>, val: serde_json::Value) -> PyResult<Py<PyAny>> {
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => b.into_py_any(py),
        serde_json::Value::String(s) => s.into_py_any(py),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => i.into_py_any(py),
            None => match n.as_f64() {
                Some(f) => f.into_py_any(py),
                None => Err(PyValueError::new_err("invalid JSON number")),
            },
        },

        serde_json::Value::Array(arr) => {
            let items: PyResult<Vec<Py<PyAny>>> =
                arr.into_iter().map(|v| value_to_py(py, v)).collect();
            Ok(items?.into_pyobject(py)?.unbind().into())
        }

        serde_json::Value::Object(obj) => {
            let dict = PyDict::new(py);
            for (k, v) in obj {
                dict.set_item(k, value_to_py(py, v)?)?;
            }
            Ok(dict.into())
        }
    }
}


pub fn py_to_value(py: Python<'_>, py_val: &Bound<'_, PyAny>) -> serde_json::Value  {

    if py_val.is_none() {
        serde_json::Value::Null   
    }

    else if py_val.is_instance_of::<PyBool>() {
        serde_json::Value::Bool(
            py_val
                .cast::<PyBool>()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        )
    }

    else if py_val.is_instance_of::<PyInt>() {
        serde_json::Value::Number(
            serde_json::Number::from(
                py_val
                    .extract::<i64>()
                    .unwrap()
            )
        )
    }

    else if py_val.is_instance_of::<PyFloat>() {
        let fv = serde_json::Number::from_f64(
            py_val
            .extract::<f64>()
            .unwrap()
        );
        match fv {
            Some(_fv) => {
                serde_json::Value::Number(_fv)
            }
            None => {
                serde_json::Value::Null
            }
        }
    }

    else if py_val.is_instance_of::<PyString>() {
        serde_json::Value::String(
            py_val
                .extract::<String>()
                .unwrap()
            )
    }

    else if py_val.is_instance_of::<PyDict>() {
        serde_json::Value::Object(
            py_val
                .cast::<PyDict>()
                .unwrap()
                .iter()
                .map(
                    |(k, v)| 
                    (
                        k.extract::<String>().unwrap(), 
                        py_to_value(py, &v)) 
                    )
                .collect()
        )
    }
    else if py_val.is_instance_of::<PyList>() {
        serde_json::Value::Array(
            py_val
                .cast::<PyList>()
                .iter()
                .map(|v| py_to_value(py, v))
                .collect()
        )
    } else {
        serde_json::Value::Null
    }
}

