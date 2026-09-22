use std::collections::HashMap;
use std::time::Duration;

use pyo3::prelude::{Bound, Py, PyAny, PyAnyMethods, PyErr, PyResult, Python, pyclass, pymethods};
use pyo3::sync::PyOnceLock;
use pyo3::types::PyBytes;

use rqx_core::response::BufferedResponse;

use super::exceptions::PyRqxError;
use super::headers::PyHeaders;
use super::py_json::value_to_py;
use super::url::PyURL;

#[pyclass]
pub struct PyResponse {
    inner: BufferedResponse,
    content_cache: PyOnceLock<Py<PyBytes>>,
    headers_cache: PyOnceLock<Py<PyHeaders>>,
    url_cache: PyOnceLock<Py<PyURL>>,
}

impl From<BufferedResponse> for PyResponse {
    fn from(inner: BufferedResponse) -> Self {
        Self {
            inner,
            content_cache: PyOnceLock::new(),
            headers_cache: PyOnceLock::new(),
            url_cache: PyOnceLock::new(),
        }
    }
}

#[pymethods]
impl PyResponse {
    #[getter]
    fn status_code(&self) -> u16 {
        self.inner.parts.status_code
    }

    #[getter]
    fn headers(&self, py: Python<'_>) -> PyResult<Py<PyHeaders>> {
        self.headers_cache
            .get_or_try_init(py, || {
                Py::new(py, PyHeaders::new(self.inner.parts.headers.clone()))
            })
            .map(|h| h.clone_ref(py))
    }

    #[getter]
    fn url(&self, py: Python<'_>) -> PyResult<Py<PyURL>> {
        self.url_cache
            .get_or_try_init(py, || Py::new(py, PyURL::new(self.inner.parts.url.clone())))
            .map(|url| url.clone_ref(py))
    }

    #[getter]
    fn elapsed(&self) -> Duration {
        self.inner.parts.elapsed
    }

    #[getter]
    fn num_retries(&self) -> u32 {
        self.inner.parts.num_retries
    }

    #[getter]
    fn retry_history(&self) -> &[(String, f64)] {
        &self.inner.parts.retry_history
    }

    #[getter]
    fn http_version(&self) -> &str {
        &self.inner.parts.http_version
    }

    #[getter]
    fn cookies(&self) -> &HashMap<String, String> {
        &self.inner.parts.cookies
    }

    #[getter]
    fn encoding_override(&self) -> &Option<String> {
        &self.inner.parts.encoding_override
    }

    #[getter]
    fn encoding(&self) -> String {
        self.inner.parts.encoding()
    }

    /// Override the encoding used by `.text`. Set to any encoding label
    /// `encoding_rs` understands ("utf-8", "iso-8859-1", "windows-1252", ...).
    /// Invalid labels silently fall back to UTF-8 when decoding.
    #[setter]
    fn set_encoding(&mut self, value: String) {
        self.inner.parts.encoding_override = Some(value);
    }

    #[getter]
    fn content(&self, py: Python<'_>) -> Py<PyBytes> {
        self.content_cache
            .get_or_init(py, || PyBytes::new(py, &self.inner.body).unbind())
            .clone_ref(py)
    }

    #[getter]
    fn text(&self) -> String {
        self.inner.text()
    }

    fn json(&self, py: Python<'_>) -> Result<Py<PyAny>, PyRqxError> {
        Ok(value_to_py(py, self.inner.json()?)?)
    }

    /// The response itself when the status is 2xx; otherwise HTTPStatusError with
    /// the response attached as `.response`.
    fn raise_for_status(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        let Some(error) = slf.borrow().inner.parts.status_error() else {
            return Ok(slf);
        };
        let error = PyErr::from(PyRqxError::from(error));
        error.value(slf.py()).setattr("response", &slf)?;
        Err(error)
    }

    #[getter]
    fn is_informational(&self) -> bool {
        self.inner.parts.is_informational()
    }

    #[getter]
    fn is_success(&self) -> bool {
        self.inner.parts.is_success()
    }

    #[getter]
    fn is_redirect(&self) -> bool {
        self.inner.parts.is_redirect()
    }

    #[getter]
    fn is_client_error(&self) -> bool {
        self.inner.parts.is_client_error()
    }

    #[getter]
    fn is_server_error(&self) -> bool {
        self.inner.parts.is_server_error()
    }

    #[getter]
    fn is_error(&self) -> bool {
        self.inner.parts.is_error()
    }
}
