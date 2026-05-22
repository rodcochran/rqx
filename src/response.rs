use bytes::Bytes;
use std::collections::HashMap;
use std::str::FromStr;

use encoding_rs::Encoding;
use http::StatusCode;
use http::header::{HeaderMap, HeaderValue};
use mime::Mime;
use pyo3::prelude::{Py, PyAny, PyResult, Python, pyclass, pymethods};
use pyo3::sync::PyOnceLock;
use pyo3::types::PyBytes;
use reqwest::Response;

use super::exceptions::{HTTPStatusError, RqxError, map_reqwest_error};
use super::headers::PyHeaders;
use super::py_json::value_to_py;

/*
Pure Rust implementation of the response parts to avoid overhead with GIL and FFI
*/
pub struct ResponseParts {
    pub(crate) status_code: u16,
    pub(crate) headers: HeaderMap, // will materialize into PyHeaders
    pub(crate) url: String,
    pub(crate) elapsed: f64,     // is f32 precise enough?
    pub(crate) num_retries: u32, // can't be negative, can use u32?
    pub(crate) retry_history: Vec<(String, f64)>,
    pub(crate) http_version: String,
    pub(crate) cookies: HashMap<String, String>,
    pub(crate) encoding_override: Option<String>,
    // What to do with content, and raw Response?
    // on PyResponse struct, we had content: Py<PyBytes>.
    // on PyStreamResposne struct, we had response: Option<reqwest::Response>,
}

impl ResponseParts {
    pub fn encoding(&self) -> String {
        if let Some(e) = &self.encoding_override {
            return e.clone();
        }
        self.detect_encoding_from_headers()
            .map(|enc| enc.name().to_lowercase())
            .unwrap_or_else(|| "utf-8".to_string())
    }

    fn resolved_encoding(&self) -> &'static Encoding {
        if let Some(label) = &self.encoding_override {
            return Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8);
        }
        &self
            .detect_encoding_from_headers()
            .unwrap_or(encoding_rs::UTF_8)
    }

    /// Pull an encoding off the Content-Type header's charset parameter.
    ///
    /// Parses the header with the `mime` crate, so we get correct handling of
    /// quoting, whitespace, multiple parameters, etc. without reinventing a
    /// MIME parser. Returns `None` if the header is missing, unparseable, has
    /// no charset parameter, or names an encoding `encoding_rs` doesn't know.
    fn detect_encoding_from_headers(&self) -> Option<&'static Encoding> {
        let content_type_str = self.content_type()?;
        let mime: Mime = Mime::from_str(content_type_str).ok()?;
        let charset = mime.get_param(mime::CHARSET)?;
        Encoding::for_label(charset.as_str().as_bytes())
    }

    fn content_type(&self) -> Option<&str> {
        self.headers.get("content-type")?.to_str().ok()
    }

    fn get_first_header_for_key(&self, key: &str) -> Option<&HeaderValue> {
        self.headers.get_all(key).iter().next()
    }

    pub fn is_informational(&self) -> bool {
        (100..200).contains(&self.status_code)
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    pub fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status_code)
            && self.get_first_header_for_key("location").is_some()
    }

    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status_code)
    }

    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status_code)
    }

    pub fn is_error(&self) -> bool {
        (400..600).contains(&self.status_code)
    }
}

impl ResponseParts {
    pub fn from_reqwest(response: &Response) -> Self {
        ResponseParts {
            status_code: response.status().as_u16(),
            headers: response.headers().clone(),
            url: response.url().to_string(),
            elapsed: 0.0,
            num_retries: 0,
            retry_history: Vec::new(),
            http_version: format!("{:?}", response.version()),
            cookies: response
                .cookies()
                .map(|c| (c.name().to_string(), c.value().to_string()))
                .collect(),
            encoding_override: None,
        }
    }
}

#[pyclass]
pub struct PyResponse {
    pub parts: ResponseParts,
    pub body: Bytes,                            // Rust source of truth
    pub content_cache: PyOnceLock<Py<PyBytes>>, // .content materialized lazily at the edge
}

#[pymethods]
impl PyResponse {
    #[getter]
    fn status_code(&self) -> u16 {
        self.parts.status_code
    }

    #[getter]
    fn headers(&self) -> PyHeaders {
        // can we figure out a way to have this referenced instead of cloned?
        PyHeaders::from_header_map(self.parts.headers.clone())
    }

    #[getter]
    fn url(&self) -> &str {
        &self.parts.url
    }

    #[getter]
    fn elapsed(&self) -> f64 {
        self.parts.elapsed
    }

    #[getter]
    fn num_retries(&self) -> u32 {
        self.parts.num_retries
    }

    #[getter]
    fn retry_history(&self) -> &[(String, f64)] {
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
            .get_or_init(py, || PyBytes::new(py, &self.body).unbind())
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
        let value = match serde_json::from_slice(&self.body) {
            Ok(v) => v,
            Err(e) => {
                let content_type = self.parts.content_type().unwrap_or("<none>");
                let preview_len = self.body.len().min(100);
                let preview = String::from_utf8_lossy(&self.body[..preview_len]);
                let ellipsis = if self.body.len() > 100 { "..." } else { "" };
                return Err(RqxError::new_err(format!(
                    "response is not JSON (HTTP {}, content-type: {}): {:?}{} ({})",
                    self.parts.status_code, content_type, preview, ellipsis, e
                )));
            }
        };

        value_to_py(py, value)
    }

    fn raise_for_status(&self) -> PyResult<()> {
        let s_result = StatusCode::from_u16(self.parts.status_code);
        match s_result {
            Ok(s) => {
                if !s.is_success() {
                    Err(HTTPStatusError::new_err(format!(
                        "{} error",
                        self.parts.status_code
                    )))
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(RqxError::new_err(format!("invalid Status Code: {e}"))),
        }
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

impl PyResponse {
    pub async fn from_response(response: Response) -> PyResult<PyResponse> {
        Ok(PyResponse {
            parts: ResponseParts::from_reqwest(&response),
            body: response.bytes().await.map_err(map_reqwest_error)?,
            content_cache: PyOnceLock::<Py<PyBytes>>::new(),
        })
    }
}
