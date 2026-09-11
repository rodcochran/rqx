use pyo3::conversion::{IntoPyObject, IntoPyObjectExt};
use pyo3::exceptions::{PyOverflowError, PyRecursionError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use serde_json::{Map, Number, Value};

pub fn value_to_py(py: Python<'_>, val: serde_json::Value) -> PyResult<Py<PyAny>> {
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => b.into_py_any(py),
        serde_json::Value::String(s) => s.into_py_any(py),
        // i64, then u64 (exact up to 2^64 - 1), then f64. Past u64 serde_json
        // already parsed the literal as f64, so precision is lost there; stdlib
        // returns an exact int. Documented divergence, see
        // https://github.com/rodcochran/rqx/issues/116.
        serde_json::Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => i.into_py_any(py),
            (None, Some(u), _) => u.into_py_any(py),
            (None, None, Some(f)) => f.into_py_any(py),
            (None, None, None) => Err(PyValueError::new_err("invalid JSON number")),
        },

        serde_json::Value::Array(arr) => {
            let items: PyResult<Vec<Py<PyAny>>> =
                arr.into_iter().map(|v| value_to_py(py, v)).collect();
            Ok(items?.into_pyobject(py)?.unbind())
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

/// The `json=` kwarg, encoded at the boundary the way stdlib `json.dumps`
/// does it (https://github.com/rodcochran/rqx/issues/118): tuples are arrays,
/// dict keys are coerced like stdlib, NaN/Inf and unsupported types raise, ints
/// past 64 bits are an OverflowError, and cycles or nesting past stdlib's
/// recursion limit raise instead of overflowing the stack.
pub struct JsonBody(Value);

/// Containers on the current encode path, by object address. Detects cycles
/// exactly and bounds depth. Linear scan is fine: the path is short.
struct EncodePath(Vec<usize>);

impl EncodePath {
    const MAX_DEPTH: usize = 1000;

    fn enter(&mut self, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let addr = obj.as_ptr() as usize;
        if self.0.contains(&addr) {
            return Err(PyValueError::new_err("Circular reference detected"));
        }
        if self.0.len() >= Self::MAX_DEPTH {
            return Err(PyRecursionError::new_err(
                "maximum recursion depth exceeded while encoding a JSON object",
            ));
        }
        self.0.push(addr);
        Ok(())
    }

    fn leave(&mut self) {
        self.0.pop();
    }
}

impl JsonBody {
    pub fn into_value(self) -> Value {
        self.0
    }

    fn encode(obj: &Bound<'_, PyAny>, path: &mut EncodePath) -> PyResult<Value> {
        if obj.is_none() {
            return Ok(Value::Null);
        }
        // bool before int: Python's bool is an int subclass.
        if let Ok(b) = obj.cast::<PyBool>() {
            return Ok(Value::Bool(b.is_true()));
        }
        if obj.is_instance_of::<PyInt>() {
            return Self::int(obj);
        }
        if let Ok(f) = obj.cast::<PyFloat>() {
            return Self::float(f.value());
        }
        if let Ok(s) = obj.cast::<PyString>() {
            return Ok(Value::String(s.to_cow()?.into_owned()));
        }
        if let Ok(dict) = obj.cast::<PyDict>() {
            path.enter(obj)?;
            let mut map = Map::with_capacity(dict.len());
            for (k, v) in dict.iter() {
                map.insert(Self::key(&k)?, Self::encode(&v, path)?);
            }
            path.leave();
            return Ok(Value::Object(map));
        }
        if let Ok(list) = obj.cast::<PyList>() {
            path.enter(obj)?;
            let items = list
                .iter()
                .map(|v| Self::encode(&v, path))
                .collect::<PyResult<_>>()?;
            path.leave();
            return Ok(Value::Array(items));
        }
        if let Ok(tuple) = obj.cast::<PyTuple>() {
            path.enter(obj)?;
            let items = tuple
                .iter()
                .map(|v| Self::encode(&v, path))
                .collect::<PyResult<_>>()?;
            path.leave();
            return Ok(Value::Array(items));
        }
        Err(PyTypeError::new_err(format!(
            "Object of type {} is not JSON serializable",
            obj.get_type().name()?
        )))
    }

    /// i64 first, then u64; serde_json's Number holds nothing wider.
    fn int(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
        if let Ok(i) = obj.extract::<i64>() {
            return Ok(Value::Number(Number::from(i)));
        }
        if let Ok(u) = obj.extract::<u64>() {
            return Ok(Value::Number(Number::from(u)));
        }
        Err(PyOverflowError::new_err(
            "Python int too large to encode as JSON (64-bit limit)",
        ))
    }

    /// Finite only, like stdlib with `allow_nan=False`.
    fn float(v: f64) -> PyResult<Value> {
        match Number::from_f64(v) {
            Some(n) => Ok(Value::Number(n)),
            None => {
                let spelling = if v.is_nan() {
                    "nan"
                } else if v > 0.0 {
                    "inf"
                } else {
                    "-inf"
                };
                Err(PyValueError::new_err(format!(
                    "Out of range float values are not JSON compliant: {spelling}"
                )))
            }
        }
    }

    /// stdlib's key coercion: str as-is, bool/None/int/float stringified.
    fn key(obj: &Bound<'_, PyAny>) -> PyResult<String> {
        if let Ok(s) = obj.cast::<PyString>() {
            return Ok(s.to_cow()?.into_owned());
        }
        if obj.is_none() {
            return Ok("null".to_owned());
        }
        if let Ok(b) = obj.cast::<PyBool>() {
            return Ok(if b.is_true() { "true" } else { "false" }.to_owned());
        }
        if obj.is_instance_of::<PyInt>() || obj.is_instance_of::<PyFloat>() {
            return Ok(obj.str()?.to_cow()?.into_owned());
        }
        Err(PyTypeError::new_err(format!(
            "keys must be str, int, float, bool or None, not {}",
            obj.get_type().name()?
        )))
    }
}

impl<'py> FromPyObject<'_, 'py> for JsonBody {
    type Error = PyErr;

    fn extract(obj: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        let mut path = EncodePath(Vec::new());
        Ok(Self(JsonBody::encode(&obj, &mut path)?))
    }
}
