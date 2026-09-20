use bytes::Bytes;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use encoding_rs::Encoding;
use http::StatusCode;
use http::header::{HeaderMap, HeaderValue};
use mime::Mime;
use pyo3::prelude::{Bound, Py, PyAny, PyAnyMethods, PyErr, PyResult, Python, pyclass, pymethods};
use pyo3::sync::PyOnceLock;
use pyo3::types::PyBytes;
use reqwest::Response;
use url::Url;

use super::exceptions::{HTTPStatusError, JSONDecodeError, map_reqwest_error};
use super::headers::PyHeaders;
use super::py_json::value_to_py;
use super::url::{PyURL, UrlReference};

use rqx_core::response::ResponseParts;

#[pyclass]
pub struct PyResponse {
    pub parts: ResponseParts,
    pub body: Bytes,                              // Rust source of truth
    pub content_cache: PyOnceLock<Py<PyBytes>>,   // .content materialized lazily at the edge
    pub headers_cache: PyOnceLock<Py<PyHeaders>>, // .headers materialized lazily at the edge
}

#[pymethods]
impl PyResponse {
    #[getter]
    fn status_code(&self) -> u16 {
        self.parts.status_code
    }

    #[getter]
    fn headers(&self, py: Python<'_>) -> PyResult<Py<PyHeaders>> {
        // Materialized once and cached — sound because a response's headers are
        // read-only. Repeat access is then a refcount bump, and
        // `resp.headers is resp.headers` holds (matching httpx).
        self.headers_cache
            .get_or_try_init(
                py,
                || {
                    Py::new(
                        py,
                        PyHeaders::from_header_map(self.parts.headers.clone()),
                    )
                },
            )
            .map(|h| h.clone_ref(py))
    }

    #[getter]
    fn url(&self, py: Python<'_>) -> PyResult<Py<PyURL>> {
        self.parts.py_url(py)
    }

    #[getter]
    fn elapsed(&self) -> Duration {
        self.parts.elapsed
    }

    #[getter]
    fn num_retries(&self) -> u32 {
        self.parts.num_retries
    }

    #[getter]
    fn retry_history(
        &self,
    ) -> &[(
        String,
        f64,
    )] {
        &self.parts.retry_history
    }

    #[getter]
    fn http_version(&self) -> &str {
        &self.parts.http_version
    }

    #[getter]
    fn cookies(&self) -> &HashMap<String, String> {
        &self.parts.cookies
    }

    #[getter]
    fn encoding_override(&self) -> &Option<String> {
        // potential to have return value &Option<str>
        &self.parts.encoding_override
    }

    #[getter]
    fn encoding(&self) -> String {
        self.parts.encoding()
    }

    /// Override the encoding used by `.text`. Set to any encoding label
    /// `encoding_rs` understands ("utf-8", "iso-8859-1", "windows-1252", ...).
    /// Invalid labels silently fall back to UTF-8 when decoding.
    #[setter]
    fn set_encoding(&mut self, value: String) {
        self.parts.encoding_override = Some(value);
    }

    #[getter]
    fn content(&self, py: Python<'_>) -> Py<PyBytes> {
        self.content_cache
            .get_or_init(
                py,
                || {
                    PyBytes::new(
                        py, &self.body,
                    )
                    .unbind()
                },
            )
            .clone_ref(py)
    }

    /// Decoded response body as a string.
    ///
    /// Resolution order for the charset:
    ///   1. `self.encoding` if the user set it explicitly
    ///   2. The `charset=` parameter on the Content-Type header
    ///   3. UTF-8 fallback
    ///
    /// Invalid byte sequences are replaced with U+FFFD rather than raising —
    /// so callers never get a panic from calling `.text`.
    #[getter]
    fn text(&self) -> String {
        let encoding = self.parts.resolved_encoding();
        let (decoded, _, _) = encoding.decode(&self.body);
        decoded.into_owned()
    }

    /// Parse the response body as JSON.
    ///
    /// Uses serde_json on the raw bytes and walks the resulting serde_json::Value
    /// into Python objects via py_json::value_to_py. This skips the stdlib
    /// json.loads round-trip (which was measurably slower than calling json.loads
    /// directly — see benchmarks/b5_json_parsing.py / docs/improvements.md).
    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let value = serde_json::from_slice(&self.body).map_err(
            |e| {
                self.parts.json_decode_error(
                    &self.body, &e,
                )
            },
        )?;
        value_to_py(
            py, value,
        )
    }

    /// The response itself when the status is 2xx; otherwise HTTPStatusError with
    /// the response attached as `.response`.
    fn raise_for_status(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        let Some(error) = slf.borrow().parts.status_error() else {
            return Ok(slf);
        };
        error.value(slf.py()).setattr(
            "response", &slf,
        )?;
        Err(error)
    }

    #[getter]
    fn is_informational(&self) -> bool {
        self.parts.is_informational()
    }

    #[getter]
    fn is_success(&self) -> bool {
        self.parts.is_success()
    }

    #[getter]
    fn is_redirect(&self) -> bool {
        self.parts.is_redirect()
    }

    #[getter]
    fn is_client_error(&self) -> bool {
        self.parts.is_client_error()
    }
    #[getter]
    fn is_server_error(&self) -> bool {
        self.parts.is_server_error()
    }
    #[getter]
    fn is_error(&self) -> bool {
        self.parts.is_error()
    }
}
