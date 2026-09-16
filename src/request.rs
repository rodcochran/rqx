use http::Method;
use http::header::{CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::PyResult;
use reqwest::{Client, Request, RequestBuilder};
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

use super::exceptions::*;
use super::request_headers::RequestHeaders;

/// The one body a request can have. `content`, `data` and `json` are three
/// ways of naming it, and at most one of them may be set.
pub enum RequestBody {
    Empty,
    Content(Vec<u8>),
    Form(HashMap<String, String>),
    Json(serde_json::Value),
}

impl RequestBody {
    pub fn new(
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
    ) -> PyResult<Self> {
        match (content, data, json) {
            (None, None, None) => Ok(Self::Empty),
            (Some(content), None, None) => Ok(Self::Content(content.to_vec())),
            (None, Some(data), None) => Ok(Self::Form(data)),
            (None, None, Some(json)) => Ok(Self::Json(json)),
            _ => Err(PyValueError::new_err(
                "Only one of content, data, or json may be set",
            )),
        }
    }

    fn apply(self, builder: RequestBuilder) -> RequestBuilder {
        match self {
            Self::Empty => builder,
            Self::Content(content) => builder.body(content),
            Self::Form(data) => builder.form(&data),
            Self::Json(json) => builder.json(&json),
        }
    }
}

/// Prototype request, never sent. Cloned per attempt and per redirect hop so
/// retries and 307/308 keep the body (https://github.com/rodcochran/rqx/issues/149).
pub struct RequestSpec {
    prototype: Request,
}

impl RequestSpec {
    pub fn from_request(request: Request) -> Self {
        Self { prototype: request }
    }

    pub fn build(
        http_client: &Client,
        method: &str,
        url: Url,
        body: RequestBody,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<&str>,
        timeout: f64,
    ) -> PyResult<Self> {
        // Uppercased like httpx, so `request("get", ...)` is GET on the wire.
        let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes())
            .map_err(|e| PyValueError::new_err(format!("invalid method {method:?}: {e}")))?;

        let mut builder = body.apply(http_client.request(method, url));

        if let Some(headers) = headers {
            builder = builder.headers(headers.into_map());
        }
        if let Some((username, password)) = auth {
            builder = builder.basic_auth(username, Some(password));
        }
        if let Some(token) = auth_bearer {
            builder = builder.bearer_auth(token);
        }

        let request = builder
            .timeout(Duration::from_secs_f64(timeout))
            .build()
            .map_err(|e| RequestError::new_err(format!("Failed to build request: {e}")))?;
        Ok(Self::from_request(request))
    }

    pub fn method(&self) -> &Method {
        self.prototype.method()
    }

    pub fn url(&self) -> &Url {
        self.prototype.url()
    }

    pub fn clone_request(&self) -> PyResult<Request> {
        self.prototype
            .try_clone()
            .ok_or_else(|| RqxError::new_err("Streaming request bodies cannot be replayed"))
    }

    pub fn redirect_target(&self, location: &str) -> PyResult<Url> {
        self.url()
            .join(location)
            .map_err(|e| RqxError::new_err(format!("Error parsing url from redirect: {e}")))
    }

    /// Next hop. Body kept when the method is kept (307/308), dropped with its
    /// headers on a downgrade to GET (302/303).
    pub fn redirected(&self, status: u16, url: Url) -> PyResult<Self> {
        let mut next = self.clone_request()?;
        let method = Self::redirect_method(next.method(), status);
        if method != *next.method() {
            *next.body_mut() = None;
            for name in [CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING] {
                next.headers_mut().remove(name);
            }
        }
        *next.method_mut() = method;
        *next.url_mut() = url;
        Ok(Self { prototype: next })
    }

    /// Matches httpx (and browsers): 302 and 303 switch every method except
    /// HEAD to GET; 301 switches only POST to GET; 307 and 308 keep the method.
    fn redirect_method(original: &Method, status_code: u16) -> Method {
        match status_code {
            302 | 303 if original != Method::HEAD => Method::GET,
            301 if original == Method::POST => Method::GET,
            _ => original.to_owned(),
        }
    }
}
