use pyo3::conversion::{IntoPyObject, IntoPyObjectExt};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::{Py, PyAny, PyResult, Python, pyclass, pymethods};
use pyo3::types::{PyDict, PyDictMethods};
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;

use super::runtime::RUNTIME;

#[pyclass]
pub struct PyClient {
    http_client: Client,
    // timeout_secs: u64,
}

#[pyclass]
pub struct PyResponse {
    #[pyo3(get)]
    status_code: u16,
    #[pyo3(get)]
    headers: HashMap<String, String>,
    #[pyo3(get)]
    content: String,
}

fn value_to_py(py: Python<'_>, val: serde_json::Value) -> PyResult<Py<PyAny>> {
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

#[pymethods]
impl PyResponse {
    fn text(&self) -> String {
        self.content.clone()
    }

    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        serde_json::from_str(&self.content)
            .map_err(|e| PyRuntimeError::new_err(format!("invalid JSON response: {e}")))
            .and_then(|v| value_to_py(py, v))
    }
}
#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (timeout))]
    fn __new__(timeout: u64) -> PyResult<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(timeout))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            // .gzip(true)
            // .brotli(true)
            .pool_max_idle_per_host(20)
            .build()
            .expect("Failed to build HTTP client");
        Ok(Self {
            http_client: http_client,
            // timeout_secs: timeout,
        })
    }

    #[pyo3(signature = (url))]
    fn get(&self, py: Python<'_>, url: &str) -> PyResult<PyResponse> {
        let request = self
            .http_client
            .get(url)
            // .timeout(Duration::from_secs(self.timeout_secs))
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("failed to build request: {e}")))?;

        let response = py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| PyRuntimeError::new_err("runtime not initialized"))?
                .block_on(async {
                    self.http_client
                        .execute(request)
                        .await
                        .map_err(|e| PyRuntimeError::new_err(format!("request failed: {e}")))
                })
        })?;

        let status_code = response.status().as_u16();

        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.to_str().unwrap_or("<non-utf8>").to_string(),
                )
            })
            .collect::<HashMap<_, _>>();

        let content = RUNTIME
            .get()
            .ok_or_else(|| PyRuntimeError::new_err("runtime not initialized"))?
            .block_on(async {
                response
                    .text()
                    .await
                    .map_err(|e| PyRuntimeError::new_err(format!("failed to read body: {e}")))
            })?;

        Ok(PyResponse {
            status_code,
            headers,
            content,
        })
    }
}
