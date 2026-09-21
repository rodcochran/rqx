use std::collections::HashMap;
use std::time::Duration;

use http::Method;
use http::header::{CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use reqwest::{Client, Request, RequestBuilder};
use url::Url;

use super::error::*;
use super::headers::Headers;
use super::query_params::QueryPairs;

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
    ) -> Result<Self, RqxError> {
        match (content, data, json) {
            (None, None, None) => Ok(Self::Empty),
            (Some(content), None, None) => Ok(Self::Content(content.to_vec())),
            (None, Some(data), None) => Ok(Self::Form(data)),
            (None, None, Some(json)) => Ok(Self::Json(json)),
            _ => Err(RequestError::RequestError(
                "Only one of content, data, or json may be set".to_string(),
            )
            .into()),
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
        mut url: Url,
        params: Option<QueryPairs>,
        body: RequestBody,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<&str>,
        timeout: f64,
    ) -> Result<Self, RqxError> {
        // Uppercased like httpx, so `request("get", ...)` is GET on the wire.
        let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes())
            .map_err(|e| RequestError::RequestError(format!("invalid method {method:?}: {e}")))?;

        if !matches!(url.scheme(), "http" | "https") {
            return Err(TransportError::UnsupportedProtocol(format!(
                "Request URL has an unsupported protocol '{}://'.",
                url.scheme()
            ))
            .into());
        }

        // `params=` replaces whatever query the URL carried, as in httpx.
        if let Some(params) = params {
            let query = params.to_string();
            url.set_query(Some(query.as_str()).filter(|q| !q.is_empty()));
        }

        let mut builder = body.apply(http_client.request(method, url));

        if let Some(headers) = headers {
            builder = builder.headers(headers.inner);
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
            .map_err(|e| RqxError::from(e))?;
        Ok(Self::from_request(request))
    }

    pub fn method(&self) -> &Method {
        self.prototype.method()
    }

    pub fn url(&self) -> &Url {
        self.prototype.url()
    }

    pub fn clone_request(&self) -> Result<Request, RqxError> {
        self.prototype.try_clone().ok_or_else(|| {
            RequestError::RequestError("Streaming request bodies cannot be replayed".to_string())
                .into()
        })
    }

    pub fn redirect_target(&self, location: &str) -> Result<Url, RqxError> {
        self.url().join(location).map_err(|e| {
            RequestError::RequestError(format!("Error parsing url from redirect: {e}")).into()
        })
    }

    /// Next hop. Body kept when the method is kept (307/308), dropped with its
    /// headers on a downgrade to GET (302/303).
    pub fn redirected(&self, status: u16, url: Url) -> Result<Self, RqxError> {
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
