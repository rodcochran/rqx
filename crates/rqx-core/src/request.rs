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

    pub fn redirected(&self, status: u16, url: Url) -> Result<Self, RqxError> {
        let method = Redirect::redirect_method(&self.method, status);
        let method_changed = method != self.method;

        let body = if method_changed {
            None
        } else {
            self.body.clone()
        };

        let mut headers = self.headers.clone();

        if let Some(headers) = headers.as_mut() {
            if method_changed {
                for name in [CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING] {
                    headers.delete_item(&name.to_string());
                }
            }

            headers.delete_item(&COOKIE.to_string());

            if !Redirect::keeps_authorization(&self.url, &url) {
                headers.delete_item(&AUTHORIZATION.to_string());
            }
        }

        Ok(Self::new(
            method,
            url,
            self.params.clone(),
            headers,
            body,
            self.auth.clone(),
            self.timeout,
            self.follow_redirects,
        ))
    }

    /*
    pub fn redirected(&self, status: u16, url: Url) -> Result<Self, RqxError> {
        let mut next = self.clone_request()?;
        let method = Self::redirect_method(next.method(), status);
        if method != next.method() {
            *next.body_mut() = None;
            for name in [CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING] {
                next.headers_mut().remove(name);
            }
        }
        next.headers_mut().remove(COOKIE);
        if !self.keeps_authorization(&url) {
            next.headers_mut().remove(AUTHORIZATION);
        }
        *next.method_mut() = method;
        *next.url_mut() = url;
        Ok(Self { prototype: next })
    }

     */
}
