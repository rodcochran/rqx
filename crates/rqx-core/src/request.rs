use http::method::Method;

use crate::auth::Auth;
use crate::error::*;
use crate::headers::Headers;
use crate::query_params::QueryPairs;
use crate::request_components::body::RequestBody;
use crate::url::reference::UrlReference;

pub struct Request {
    pub method: Method,
    pub url: UrlReference,
    pub params: Option<QueryPairs>,
    pub headers: Option<Headers>,
    pub body: RequestBody,
    pub auth: Option<Auth>,
    pub timeout: Option<f64>,
    pub follow_redirects: Option<bool>,
}

impl Request {
    pub fn new(
        method: &str,
        url: UrlReference,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        body: RequestBody,
        auth: Option<Auth>,
        timeout: Option<f64>,
        follow_redirects: Option<bool>,
    ) -> Result<Self, RqxError> {
        let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes())
            .map_err(|e| RqxError::InvalidArgument(format!("invalid method {method:?}: {e}")))?;

        Ok(Self {
            method,
            url,
            params,
            headers,
            body,
            auth,
            timeout,
            follow_redirects,
        })
    }
}
