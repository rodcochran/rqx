use http::method::Method;
use url::Url;

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

// TODO: implement Redirect Object
// ex: Redirect { status, location }

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
}
