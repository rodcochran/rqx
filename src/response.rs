use std::collections::HashMap;
use std::str::FromStr;

use encoding_rs::Encoding;
use http::StatusCode;
use mime::Mime;
use pyo3::prelude::{Py, PyAny, PyResult, Python, pyclass, pymethods};
use pyo3::types::PyBytes;
use reqwest::Response;

use super::exceptions::{RqxError, HTTPStatusError, map_reqwest_error};
use super::headers::PyHeaders;
use super::py_json::value_to_py;
use super::runtime::RUNTIME;

#[pyclass]
pub struct PyResponse {
    #[pyo3(get)]
    pub status_code: u16,

    #[pyo3(get)]
    pub headers: Py<PyHeaders>,

    #[pyo3(get)]
    pub content: Py<PyBytes>,

    #[pyo3(get)]
    pub url: String,

    #[pyo3(get)]
    pub(crate) elapsed: f64,

    #[pyo3(get)]
    pub(crate) num_retries: i32,

    #[pyo3(get)]
    pub(crate) retry_history: Vec<(String, f64)>,

    #[pyo3(get)]
    pub(crate) http_version: String,

    #[pyo3(get)]
    pub(crate) cookies: HashMap<String, String>,

    /// User-set encoding override. `None` means "auto-detect from headers,
    /// fall back to UTF-8". Exposed to Python via the `encoding` getter/setter.
    pub(crate) encoding_override: Option<String>,
}

#[pymethods]
impl PyResponse {
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
    fn text(&self, py: Python<'_>) -> String {
        let bytes = self.content.as_bytes(py);
        let encoding = self.resolved_encoding(py);
        let (decoded, _, _) = encoding.decode(bytes);
        decoded.into_owned()
    }

    /// Resolved character encoding name for the response body.
    ///
    /// Returns the user override if one was set, otherwise the charset from
    /// the Content-Type header, otherwise `"utf-8"`. Names are lowercased to
    /// match httpx's convention.
    #[getter]
    fn encoding(&self, py: Python<'_>) -> String {
        if let Some(e) = &self.encoding_override {
            return e.clone();
        }
        let headers = self.headers.borrow(py);
        detect_encoding_from_headers(&headers)
            .map(|enc| enc.name().to_lowercase())
            .unwrap_or_else(|| "utf-8".to_string())
    }

    /// Override the encoding used by `.text`. Set to any encoding label
    /// `encoding_rs` understands ("utf-8", "iso-8859-1", "windows-1252", ...).
    /// Invalid labels silently fall back to UTF-8 when decoding.
    #[setter]
    fn set_encoding(&mut self, value: String) {
        self.encoding_override = Some(value);
    }

    /// Parse the response body as JSON.
    ///
    /// Uses serde_json on the raw bytes and walks the resulting serde_json::Value
    /// into Python objects via py_json::value_to_py. This skips the stdlib
    /// json.loads round-trip (which was measurably slower than calling json.loads
    /// directly — see benchmarks/b5_json_parsing.py / docs/improvements.md).
    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let value: serde_json::Value = serde_json::from_slice(self.content.as_bytes(py))
            .map_err(|e| RqxError::new_err(format!("invalid JSON response: {e}")))?;
        value_to_py(py, value)
    }

    fn raise_for_status(&self) -> PyResult<()> {
        let s_result = StatusCode::from_u16(self.status_code);
        match s_result {
            Ok(s) => {
                if !s.is_success() {
                    Err(HTTPStatusError::new_err(format!("{} error", self.status_code)))
                }
                else {
                    Ok(())
                }
            }
            Err(e) => {
                Err(RqxError::new_err(format!("invalid Status Code: {e}")))
            }
        }
    }

    /// `True` for 1xx responses.
    #[getter]
    fn is_informational(&self) -> bool {
        (100..200).contains(&self.status_code)
    }

    /// `True` for 2xx responses.
    #[getter]
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    /// `True` for 3xx responses that carry a `Location` header.
    ///
    /// Mirrors httpx semantics: a 3xx without a Location header (e.g. 304
    /// Not Modified) is not a redirect that can be followed, so it's not
    /// classified as one here either.
    #[getter]
    fn is_redirect(&self, py: Python<'_>) -> bool {
        (300..400).contains(&self.status_code)
            && self.headers.borrow(py).get_first("location").is_some()
    }

    /// `True` for 4xx responses.
    #[getter]
    fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status_code)
    }

    /// `True` for 5xx responses.
    #[getter]
    fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status_code)
    }

    /// `True` for any 4xx or 5xx response.
    #[getter]
    fn is_error(&self) -> bool {
        (400..600).contains(&self.status_code)
    }
}

impl PyResponse {
    /// Resolve the encoding to use for `.text` decoding, with full fallback chain.
    fn resolved_encoding(&self, py: Python<'_>) -> &'static Encoding {
        if let Some(label) = &self.encoding_override {
            return Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8);
        }
        let headers = self.headers.borrow(py);
        detect_encoding_from_headers(&headers).unwrap_or(encoding_rs::UTF_8)
    }
}

/// Pull an encoding off the Content-Type header's charset parameter.
///
/// Parses the header with the `mime` crate, so we get correct handling of
/// quoting, whitespace, multiple parameters, etc. without reinventing a
/// MIME parser. Returns `None` if the header is missing, unparseable, has
/// no charset parameter, or names an encoding `encoding_rs` doesn't know.
fn detect_encoding_from_headers(headers: &PyHeaders) -> Option<&'static Encoding> {
    let content_type = headers.get_first("content-type")?;
    let mime: Mime = Mime::from_str(content_type).ok()?;
    let charset = mime.get_param(mime::CHARSET)?;
    Encoding::for_label(charset.as_str().as_bytes())
}

impl PyResponse {
    pub fn from_response(py: Python<'_>, response: Response) -> PyResult<PyResponse> {
        let status_code = response.status().as_u16();

        // Collect into Vec to preserve insertion order and duplicate keys
        // (e.g. Set-Cookie). PyHeaders does case-insensitive lookup on top.
        let header_pairs: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.to_str().unwrap_or("<non-utf8>").to_string(),
                )
            })
            .collect();
        let headers = Py::new(py, PyHeaders::from_pairs(header_pairs))?;

        let url = response.url().as_str().to_owned();
        let http_version  = format!("{:?}", response.version());
        let cookies: HashMap<String, String> = response.cookies()
            .map(|c| (
                c.name().to_string(),
                c.value().to_string())
            )
            .collect();

        let body = py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| RqxError::new_err("runtime not initialized"))?
                .block_on(async {
                    response.bytes().await.map_err(map_reqwest_error)
                })
        })?;
        // Build PyBytes once under the GIL. No Vec<u8> intermediate; callers get
        // the same Python bytes object on every access instead of a fresh clone.
        let content = PyBytes::new(py, &body).unbind();

        Ok(
            PyResponse  {
                status_code: status_code,
                headers: headers,
                content: content,
                url: url,
                elapsed: 0.0,
                num_retries: 0,
                retry_history: Vec::new(),
                http_version: http_version,
                cookies: cookies,
                encoding_override: None,
            }
        )

    }

    pub async fn from_response_async(response: Response) -> PyResult<PyResponse> {
        let status_code = response.status().as_u16();

        let header_pairs: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.to_str().unwrap_or("<non-utf8>").to_string(),
                )
            })
            .collect();

        let url = response.url().as_str().to_owned();
        let http_version  = format!("{:?}", response.version());
        let cookies: HashMap<String, String> = response.cookies()
            .map(|c| (
                c.name().to_string(),
                c.value().to_string())
            )
            .collect();

        let body = response.bytes().await.map_err(map_reqwest_error)?;
        // Briefly acquire the GIL to allocate a Python bytes object directly
        // from the response body. No .await crosses this closure so there's
        // no deadlock risk with the GIL acquire that pyo3-async-runtimes
        // performs later to dispatch our result back to Python.
        //
        // This is one of three viable ways to get the body into Python:
        //
        //   1. (current) Build Py<PyBytes> here, inside the async future.
        //      Cost: one extra GIL acquire per response (this one here,
        //      plus the one pyo3-async-runtimes already does when it
        //      resolves the Python future). Under heavy completion storms
        //      those two acquires can contend for the same lock.
        //
        //   2. Return an intermediate struct holding bytes::Bytes from the
        //      future, and `impl IntoPyObject` on it so PyBytes is built
        //      during the conversion step — which runs inside the GIL
        //      acquire that pyo3-async-runtimes already does. Zero extra
        //      acquires per response; requires a two-struct split and an
        //      IntoPyObject impl that has to live alongside the pyclass.
        //
        //   3. Keep the body as Vec<u8> on the response struct, and clone
        //      to Python bytes on every `.content` access. Zero extra
        //      acquires in the future, but every access allocates, and the
        //      body lives on both heaps for its lifetime (roughly 2x RSS
        //      on the response path).
        //
        // Picked (1) because it's simpler than (2) while keeping the
        // single-allocation property of (3). If profiling ever shows the
        // double GIL acquire showing up as tail latency at high concurrency,
        // switch to (2).
        let (content, headers) = Python::attach(|py| -> PyResult<(Py<PyBytes>, Py<PyHeaders>)> {
            let content = PyBytes::new(py, &body).unbind();
            let headers = Py::new(py, PyHeaders::from_pairs(header_pairs))?;
            Ok((content, headers))
        })?;

        Ok(
            PyResponse  {
                status_code: status_code,
                headers: headers,
                content: content,
                url: url,
                elapsed: 0.0,
                num_retries: 0,
                retry_history: Vec::new(),
                http_version: http_version,
                cookies: cookies,
                encoding_override: None,
            }
        )

    }
}