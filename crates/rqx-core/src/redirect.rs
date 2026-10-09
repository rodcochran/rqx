use http::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, TRANSFER_ENCODING};
use http::method::Method;
use url::Url;

use crate::error::*;

const DEFAULT_MAX_REDIRECTS: u32 = 20;
const DEFAULT_FOLLOW_REDIRECTS: bool = false;
const DEFAULT_RAISE_ON_REDIRECT: bool = true;

#[derive(Clone, Copy)]
pub struct RedirectPolicy {
    pub follow: bool,
    pub max_redirects: u32,
    pub raise_on_exceeded: bool,
}

impl RedirectPolicy {
    pub fn with_defaults(
        follow: Option<bool>,
        max_redirects: Option<u32>,
        raise_on_exceeded: Option<bool>,
    ) -> Self {
        let defaults = RedirectPolicy::default();
        Self {
            follow: follow.unwrap_or(defaults.follow),
            max_redirects: max_redirects.unwrap_or(defaults.max_redirects),
            raise_on_exceeded: raise_on_exceeded.unwrap_or(defaults.raise_on_exceeded),
        }
    }
}

impl Default for RedirectPolicy {
    fn default() -> Self {
        RedirectPolicy {
            follow: DEFAULT_FOLLOW_REDIRECTS,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            raise_on_exceeded: DEFAULT_RAISE_ON_REDIRECT,
        }
    }
}

pub struct Redirect {}

impl Redirect {
    pub fn redirect_method(original: &Method, status_code: u16) -> Method {
        match status_code {
            302 | 303 if original != Method::HEAD => Method::GET,
            301 if original == Method::POST => Method::GET,
            _ => original.to_owned(),
        }
    }

    pub fn keeps_authorization(from: &Url, next: &Url) -> bool {
        match (from.port_or_known_default(), next.port_or_known_default()) {
            (Some(80), Some(443)) if (from.scheme(), next.scheme()) == ("http", "https") => {
                from.host() == next.host()
            }
            _ => from.origin() == next.origin(),
        }
    }

    pub fn redirect_target(url: &Url, location: &str) -> Result<Url, RqxError> {
        url.join(location)
            .map_err(|e| RqxError::RequestError(format!("Error parsing url from redirect: {e}")))
    }

    pub fn redirected_request(
        mut request: reqwest::Request,
        status: u16,
        url: Url,
    ) -> reqwest::Request {
        let method_for_redirect = Self::redirect_method(request.method(), status);
        if method_for_redirect != request.method() {
            *request.body_mut() = None;
            for name in [CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING] {
                request.headers_mut().remove(name);
            }
        }
        request.headers_mut().remove(COOKIE);

        if !Self::keeps_authorization(request.url(), &url) {
            request.headers_mut().remove(AUTHORIZATION);
        };

        *request.method_mut() = method_for_redirect;
        *request.url_mut() = url;
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keeps_authorization(from: &str, to: &str) -> bool {
        let from_url = Url::parse(from).unwrap();
        let to_url = Url::parse(to).unwrap();
        Redirect::keeps_authorization(&from_url, &to_url)
    }

    #[test]
    fn same_origin_keeps_authorization() {
        assert!(keeps_authorization("http://a/", "http://a:80/x"));
    }

    #[test]
    fn https_upgrade_on_default_ports_keeps_authorization() {
        assert!(keeps_authorization("http://a/", "https://a/"));
    }

    #[test]
    fn other_host_drops_authorization() {
        assert!(!keeps_authorization("http://a/", "https://b/"));
    }

    #[test]
    fn upgrade_from_non_default_port_drops_authorization() {
        assert!(!keeps_authorization("http://a:8080/", "https://a/"));
    }

    #[test]
    fn https_downgrade_drops_authorization() {
        assert!(!keeps_authorization("https://a/", "http://a/"));
    }
}
