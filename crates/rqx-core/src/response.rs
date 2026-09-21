use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use encoding_rs::Encoding;
use http::StatusCode;
use http::header::{HeaderMap, HeaderValue};
use mime::Mime;
use reqwest::Response;
use url::Url;

use super::error::*;
use super::url::PyURL;

use crate::url::reference::UrlReference;

/// Headers received, body unread. Everything known before the body — status,
/// headers, cookies, retry telemetry, elapsed — lives in `parts`. `read` buffers
/// the body into a `PyResponse`; stream responses take the live body as-is.
pub struct PendingResponse {
    pub parts: ResponseParts,
    response: Response,
}

impl PendingResponse {
    pub fn new(response: Response) -> Self {
        Self {
            parts: ResponseParts::from_reqwest(&response),
            response,
        }
    }

    pub fn with_retries(mut self, num_retries: u32, retry_history: Vec<(String, f64)>) -> Self {
        self.parts.num_retries = num_retries;
        self.parts.retry_history = retry_history;
        self
    }

    /// Buffer the body. The one place a `PyResponse` is built from the wire.
    pub async fn read(self) -> PyResult<PyResponse> {
        Ok(PyResponse {
            parts: self.parts,
            body: self.response.bytes().await.map_err(map_reqwest_error)?,
            content_cache: PyOnceLock::new(),
            headers_cache: PyOnceLock::new(),
        })
    }

    /// Consume the body without keeping it, so the connection goes back to the pool.
    pub async fn drain(self) {
        let _ = self.response.bytes().await;
    }

    /// Parts plus the live body, for stream responses.
    pub fn into_parts(self) -> (ResponseParts, Response) {
        (self.parts, self.response)
    }
}

/*
Pure Rust implementation of the response parts to avoid overhead with GIL and FFI
*/
pub struct ResponseParts {
    pub status_code: u16,
    pub headers: HeaderMap, // will materialize into Headers
    pub url: Url,
    pub url_cache: PyOnceLock<Py<PyURL>>,
    pub elapsed: Duration,
    pub num_retries: u32, // can't be negative, can use u32?
    pub retry_history: Vec<(String, f64)>,
    pub http_version: String,
    pub cookies: HashMap<String, String>,
    pub encoding_override: Option<String>,
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

    pub fn resolved_encoding(&self) -> &'static Encoding {
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

    pub fn content_type(&self) -> Option<&str> {
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

    /// `raise_for_status()`'s error for any status outside 2xx, with httpx's exact
    /// message. The caller attaches the response as `.response`.
    pub fn status_error(&self) -> Option<RqxError> {
        if self.is_success() {
            return None;
        }
        let kind = match self.status_code / 100 {
            1 => "Informational response",
            3 => "Redirect response",
            4 => "Client error",
            5 => "Server error",
            _ => "Invalid status code",
        };
        let reason = StatusCode::from_u16(self.status_code)
            .ok()
            .and_then(|s| s.canonical_reason())
            .unwrap_or("");
        let code = self.status_code;
        let mut message = format!("{kind} '{code} {reason}' for url '{}'\n", self.url);
        let location = self.headers.get(http::header::LOCATION);
        if let (301 | 302 | 303 | 307 | 308, Some(location)) = (code, location) {
            let location = String::from_utf8_lossy(location.as_bytes());
            message.push_str(&format!("Redirect location: '{location}'\n"));
        }
        message.push_str(&format!(
            "For more information check: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/{code}"
        ));
        Some(HTTPError::HTTPStatusError(message).into())
    }

    /// `json()`'s error for a body serde_json rejects, positioned the way the stdlib
    /// parser positions it: `pos` counts characters, not bytes.
    pub fn json_decode_error(&self, body: &[u8], error: &serde_json::Error) -> PyErr {
        let doc = String::from_utf8_lossy(body).into_owned();
        let byte_offset = if error.line() == 0 {
            0
        } else {
            let line_start: usize = body
                .split(|b| *b == b'\n')
                .take(error.line() - 1)
                .map(|line| line.len() + 1)
                .sum();
            (line_start + error.column().saturating_sub(1)).min(body.len())
        };
        let pos = String::from_utf8_lossy(&body[..byte_offset])
            .chars()
            .count();
        let full = error.to_string();
        let position = format!(" at line {} column {}", error.line(), error.column());
        let reason = full.strip_suffix(&position).unwrap_or(&full);
        let content_type = self.content_type().unwrap_or("<none>");
        let message = format!(
            "response is not JSON (HTTP {}, content-type: {content_type}): {reason}",
            self.status_code
        );
        JSONDecodeError::new_err((message, doc, pos))
    }
}

impl ResponseParts {
    /// Materialized once and cached, like `headers`: a response's URL is
    /// read-only, so `resp.url is resp.url` holds.
    pub fn py_url(&self, py: Python<'_>) -> PyResult<Py<PyURL>> {
        self.url_cache
            .get_or_try_init(py, || {
                Py::new(py, PyURL::new(UrlReference::from_url(self.url.clone())))
            })
            .map(|url| url.clone_ref(py))
    }

    pub fn from_reqwest(response: &Response) -> Self {
        ResponseParts {
            status_code: response.status().as_u16(),
            headers: response.headers().clone(),
            url: response.url().clone(),
            url_cache: PyOnceLock::new(),
            elapsed: Duration::ZERO,
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
