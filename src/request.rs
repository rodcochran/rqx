use http::Method;
use http::header::{CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::PyResult;
use reqwest::{Client, Request};
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

use super::exceptions::*;

/// Prototype request, never sent. Cloned per attempt and per redirect hop so
/// retries and 307/308 keep the body (https://github.com/rodcochran/rqx/issues/149).
pub struct RequestSpec {
    prototype: Request,
}

impl RequestSpec {
    pub fn from_request(request: Request) -> Self {
        Self { prototype: request }
    }

    pub fn method(&self) -> &Method {
        self.prototype.method()
    }

    pub fn url(&self) -> &Url {
        self.prototype.url()
    }

    pub fn build(&self) -> PyResult<Request> {
        self.prototype
            .try_clone()
            .ok_or_else(|| RqxError::new_err("Streaming request bodies cannot be replayed"))
    }

    /// Next hop. Body kept when the method is kept (307/308), dropped with its
    /// headers on a downgrade to GET (302/303).
    pub fn redirected(&self, status: u16, url: Url) -> PyResult<Self> {
        let mut next = self.build()?;
        let method = determine_redirect_method(next.method(), status);
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
}

pub fn build_client_request(
    http_client: &Client,
    method: &str,
    url: &str,
    content: Option<&[u8]>,
    data: Option<HashMap<String, String>>,
    json: Option<&serde_json::Value>,
    params: Option<HashMap<String, String>>,
    headers: Option<HashMap<String, String>>,
    auth: Option<(String, String)>,
    auth_bearer: Option<&str>,
    timeout: f64,
) -> PyResult<Request> {
    let mut builder = http_client.request(Method::from_bytes(method.as_bytes()).unwrap(), url);

    let count = [content.is_some(), data.is_some(), json.is_some()]
        .into_iter()
        .filter(|b| *b)
        .count();

    if count > 1 {
        return Err(PyValueError::new_err(
            "Only one of content, data, or json may be set",
        ));
    }

    if let Some(c) = content {
        builder = builder.body(c.to_vec())
    };

    if let Some(d) = data {
        builder = builder.form(&d)
    }

    if let Some(j) = json {
        builder = builder.json(j)
    };

    if let Some(p) = params {
        builder = builder.query(&p)
    };

    if let Some(h) = headers {
        builder = builder.headers((&h).try_into().expect("valid headers"))
    };

    if let Some(a) = auth {
        builder = builder.basic_auth(a.0, Some(a.1))
    }

    if let Some(token) = auth_bearer {
        builder = builder.bearer_auth(token);
    }

    builder = builder.timeout(Duration::from_secs_f64(timeout));

    let request = builder
        .build()
        .map_err(|e| RqxError::new_err(format!("Failed to build request: {e}")))?;

    return Ok(request);
}

/// Pick the request method to use when following an HTTP redirect.
///
/// Per RFC 7231 §6.4, 302 and 303 responses conventionally cause the client
/// to switch to GET on the follow-up request (except for HEAD, which stays
/// HEAD). That's what we implement here.
///
/// 301 preserves the method for now; httpx downgrades it to GET.
pub fn determine_redirect_method(original_method: &Method, status_code: u16) -> Method {
    if (status_code == 302 || status_code == 303) && original_method != Method::HEAD {
        Method::GET
    } else {
        original_method.to_owned()
    }
}

pub fn determine_redirect_url(current_url: &Url, location: &str) -> PyResult<Url> {
    Ok(current_url.join(location).unwrap())
}
