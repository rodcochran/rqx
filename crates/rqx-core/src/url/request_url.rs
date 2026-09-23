use url::Url;

use super::client_url::RqxClientUrl;
use super::reference::UrlReference;
use crate::error::RqxError;

/// A `Client(base_url=)`, canonicalized.
///
/// The trailing `/` is forced so RFC 3986 join gives what users expect:
/// `base_url + "/users"` resolves under the base path rather than dropping
/// its last segment. Same normalization httpx does at construction.
#[derive(Clone)]
pub struct BaseUrl(Url);

impl BaseUrl {
    pub fn new(url: &RqxClientUrl) -> Result<Self, RqxError> {
        match url.get_inner() {
            UrlReference::Absolute(base) => Ok(Self(base).with_trailing_slash()),
            UrlReference::Relative(_) => Err(RqxError::InvalidURL(format!(
                "invalid base_url \"{url}\": relative URL without a base"
            ))),
        }
    }

    fn with_trailing_slash(mut self) -> Self {
        if !self.0.path().ends_with('/') {
            self.0.set_path(&format!("{}/", self.0.path()));
        }
        self
    }

    pub fn get_inner(&self) -> Url {
        self.0.clone()
    }

    // A reference contributes its path and query only. An authority it carries
    // (`//other.example/x`) is not a host rqx will target, as in httpx.
    pub fn join(&self, url: &RqxClientUrl) -> Result<Url, RqxError> {
        match self.0.join(url.raw_path().trim_start_matches('/')) {
            Ok(joined) => Ok(joined),
            Err(e) => Err(RqxError::InvalidURL(format!(
                "could not join base_url with \"{url}\": {e}"
            ))),
        }
    }
}
