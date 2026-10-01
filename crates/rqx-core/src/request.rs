use http::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, TRANSFER_ENCODING};
use http::method::Method;
use url::Url;

use crate::auth::Auth;
use crate::error::RqxError;
use crate::headers::Headers;
use crate::query_params::QueryPairs;
use crate::redirect::Redirect;
use crate::request_components::body::RequestBody;

pub struct Request {
    pub method: Method,
    pub url: Url,
    pub params: Option<QueryPairs>,
    pub headers: Option<Headers>,
    pub body: Option<RequestBody>,
    pub auth: Option<Auth>,
    pub timeout: Option<f64>,
    pub follow_redirects: Option<bool>,
}

impl Request {
    pub fn new(
        method: Method,
        url: Url,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        body: Option<RequestBody>,
        auth: Option<Auth>,
        timeout: Option<f64>,
        follow_redirects: Option<bool>,
    ) -> Self {
        Self {
            method,
            url,
            params,
            headers,
            body,
            auth,
            timeout,
            follow_redirects,
        }
    }

    pub fn redirected(mut self, status: u16, url: Url) -> Self {
        let method_for_redirect = Redirect::redirect_method(&self.method, status);

        if method_for_redirect != self.method {
            self.body = None;
        }

        if let Some(headers) = self.headers.as_mut() {
            if method_for_redirect != self.method {
                for name in [CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING] {
                    headers.delete_item_safe(name.as_ref());
                }
            }

            headers.delete_item_safe(COOKIE.as_ref());

            if !Redirect::keeps_authorization(&self.url, &url) {
                headers.delete_item_safe(AUTHORIZATION.as_ref());
            }
        }

        self.method = method_for_redirect;
        self.url = url;
        self
    }
}
