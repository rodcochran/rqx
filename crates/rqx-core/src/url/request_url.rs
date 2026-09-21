use url::{ParseError, Url};

use super::reference::UrlReference;
use crate::error::*;

use crate::query_params::QueryPairs;

/// A `Client(base_url=)`, canonicalized.
///
/// The trailing `/` is forced so RFC 3986 join gives what users expect:
/// `base_url + "/users"` resolves under the base path rather than dropping
/// its last segment. Same normalization httpx does at construction.
#[derive(Clone)]
pub struct BaseUrl(Url);

impl BaseUrl {
    pub fn parse(input: &str) -> Result<Self, RqxError> {
        let mut url = Url::parse(input)
            .map_err(|e| RqxError::InvalidURL(format!("invalid base_url {input:?}: {e}")))?;
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        Ok(Self(url))
    }

    pub fn get_inner(&self) -> Url {
        self.0.clone()
    }
}

pub enum RequestUrl {
    Url(String),
}

impl RequestUrl {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Url(url) => url,
        }
    }

    pub fn resolve(
        &self,
        base: Option<&BaseUrl>,
        params: Option<QueryPairs>,
    ) -> Result<Url, RqxError> {
        let absolute = match Url::parse(self.as_str()) {
            Ok(url) if url.has_authority() => Some(url),
            Ok(_) | Err(ParseError::RelativeUrlWithoutBase) => None,
            Err(e) => {
                return Err(RqxError::InvalidURL(format!(
                    "invalid URL {:?}: {e}",
                    self.as_str()
                )));
            }
        };

        let mut url = match (absolute, base) {
            (Some(url), _) => url,
            (None, Some(base)) => {
                // A reference contributes its path and query only. An authority
                // it carries (`//other.example/x`) is not a host rqx will
                // target, which is how httpx merges it too.
                let path = UrlReference::parse(self.as_str())?.raw_path();
                base.0.join(path.trim_start_matches('/')).map_err(|e| {
                    RqxError::InvalidURL(format!(
                        "could not join base_url with {:?}: {e}",
                        self.as_str()
                    ))
                })?
            }
            (None, None) => {
                return Err(TransportError::UnsupportedProtocol(
                    "Request URL is missing an 'http://' or 'https://' protocol.".to_string(),
                )
                .into());
            }
        };

        // `params=` replaces whatever query the URL carried, as in httpx.
        if let Some(params) = params {
            let query = params.to_string();
            url.set_query(Some(query.as_str()).filter(|q| !q.is_empty()));
        }

        match url.scheme() {
            "http" | "https" => Ok(url),
            scheme => Err(TransportError::UnsupportedProtocol(format!(
                "Request URL has an unsupported protocol '{scheme}://'."
            ))
            .into()),
        }
    }
}
