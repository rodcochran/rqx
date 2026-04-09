use pyo3::conversion::{IntoPyObject, IntoPyObjectExt};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::{Py, PyAny, PyResult, Python, PyRef, pyclass, pymethods};
use pyo3::types::{PyAnyMethods, PyBool, PyDict, PyDictMethods, PyFloat, PyInt, PyList, PyString};
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use pyo3::Bound;

use super::runtime::RUNTIME;

use http::{Method, StatusCode};

const DEFAULT_TIMEOUT: u64 = 15;


#[pyclass]
pub struct PyClient {
    http_client: Client,
    timeout_secs: u64,
}

#[pyclass]
pub struct PyResponse {
    #[pyo3(get)]
    status_code: u16,
    #[pyo3(get)]
    headers: HashMap<String, String>,
    #[pyo3(get)]
    content: Vec<u8>,
}

#[pymethods]
impl PyResponse {
    fn text(&self) -> String {
        // might want to revisit this... particularly the unwrap()
        std::str::from_utf8(&self.content).unwrap().to_string()
    }

    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        serde_json::from_str(&self.text())
            .map_err(|e| PyRuntimeError::new_err(format!("invalid JSON response: {e}")))
            .and_then(|v| value_to_py(py, v))
    }

    fn raise_for_status(&self) -> PyResult<()> {
        let s_result = StatusCode::from_u16(self.status_code);
        match s_result {
            Ok(s) => {
                if !s.is_success() {
                    Err(PyRuntimeError::new_err(()))
                }
                else {
                    Ok(())
                }
            }
            Err(e) => {
                Err(PyRuntimeError::new_err(format!("invalid Status Code: {e}")))
            }
        }
    }
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


fn py_to_value(py: Python<'_>, py_val: &Bound<'_, PyAny>) -> serde_json::Value  {

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


#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (timeout=None))]
    fn __new__(
        timeout: Option<u64>
    ) -> PyResult<Self> {
        let timeout_secs = timeout.unwrap_or(DEFAULT_TIMEOUT);
        let http_client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            //.connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            // .gzip(true)
            // .brotli(true)
            .pool_max_idle_per_host(20)
            .build()
            .expect("Failed to build HTTP client");
        Ok(Self {
            http_client: http_client,
            timeout_secs: timeout_secs,
        })
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) {
        // No-op exit since Reqwest client manages an Arc internally.
    }
    /*
    class Client
        def request(
            self,
            method: str,
            url: URL | str,
            *,
            content: RequestContent | None = None,
            data: RequestData | None = None,
            files: RequestFiles | None = None,
            json: typing.Any | None = None,
            params: QueryParamTypes | None = None,
            headers: HeaderTypes | None = None,
            cookies: CookieTypes | None = None,
            auth: AuthTypes | UseClientDefault | None = USE_CLIENT_DEFAULT,
            follow_redirects: bool | UseClientDefault = USE_CLIENT_DEFAULT,
            timeout: TimeoutTypes | UseClientDefault = USE_CLIENT_DEFAULT,
            extensions: RequestExtensions | None = None,
        ) -> Response:
     */
    #[pyo3(
        signature = (
            method, 
            url, 
            // content, 
            // data, 
            // files, 
            json=None, 
            params=None, 
            headers=None, 
            // cookies, 
            // auth, 
            // follow_redirects, 
            timeout=None
            // extensions
        )
    )]
    fn request(
        &self, 
        py: Python<'_>, 
        method: &str, 
        url: &str, 
        // content: &str,
        // data: &Bound<'_, PyDict>,
        // files: &Bound<'_, PyDict>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        // cookies: &Bound<'_, PyDict>,
        // auth: Option<String>,
        // follow_redirects: Option<bool>,
        timeout: Option<u64>,
        // extensions: &Bound<'_, PyDict>,
    ) -> PyResult<PyResponse> {
        let mut builder = self.http_client
            .request(Method::from_bytes(method.as_bytes()).unwrap(), url);

        if let Some(j) = json {
            builder = builder
                .json(&py_to_value(py, j))
        };

        if let Some(p) = params {
            builder = builder
                .query(&p)
        };
        
        if let Some(h) = headers {
            builder = builder
                .headers((&h).try_into().expect("valid headers"))
        };

        if let Some(t) = timeout {
            builder = builder
                .timeout(Duration::from_secs(t))
        };

        let request = builder
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to build request: {e}")))?;

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

        let content = py
            .detach(|| {
                RUNTIME
                    .get()
                    .ok_or_else(|| PyRuntimeError::new_err("runtime not initialized"))?
                    .block_on(async {
                        response.bytes().await.map_err(|e| {
                            PyRuntimeError::new_err(format!("failed to read body: {e}"))
                        })
                    })
            })?
            .to_vec();

        Ok(PyResponse {
            status_code,
            headers,
            content,
        })
    }

    #[pyo3(signature = (url, params=None, headers=None, timeout=None))]
    fn get(
        &self, 
        py: Python<'_>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "GET", url, None, params, headers, timeout)
    }

    #[pyo3(signature = (url, params=None, headers=None, timeout=None))]
    fn options(
        &self, 
        py: Python<'_>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "OPTIONS", url, None, params, headers, timeout)
    }

    #[pyo3(signature = (url, params=None, headers=None, timeout=None))]
    fn head(
        &self, 
        py: Python<'_>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "HEAD", url, None, params, headers, timeout)
    }


    #[pyo3(signature = (url, json=None, params=None, headers=None, timeout=None))]
    fn post(
        &self, 
        py: Python<'_>, 
        url: &str,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "POST", url, json, params, headers, timeout)
    }

    #[pyo3(signature = (url,json=None, params=None, headers=None, timeout=None))]
    fn put(
        &self, 
        py: Python<'_>, 
        url: &str,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "PUT", url, json, params, headers, timeout)
    }

    #[pyo3(signature = (url, json=None, params=None, headers=None, timeout=None))]
    fn patch(
        &self, 
        py: Python<'_>, 
        url: &str,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "PATCH", url, json, params, headers, timeout)
    }

    #[pyo3(signature = (url, params=None, headers=None, timeout=None))]
    fn delete(
        &self, 
        py: Python<'_>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "DELETE", url, None, params, headers, timeout)
    }
}
