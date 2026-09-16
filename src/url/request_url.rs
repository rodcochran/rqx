use pyo3::prelude::*;
use pyo3::types::PyString;
use url::{ParseError, Url};

use super::py_url::PyURL;
use super::reference::UrlReference;
use crate::exceptions::{InvalidURL, UnsupportedProtocol};
use crate::query_params::QueryPairs;

/// A `Client(base_url=)`, canonicalized.
///
/// The trailing `/` is forced so RFC 3986 join gives what users expect:
/// `base_url + "/users"` resolves under the base path rather than dropping
/// its last segment. Same normalization httpx does at construction.
#[derive(Clone)]
pub struct BaseUrl(Url);

impl BaseUrl {
    pub fn parse(input: &str) -> PyResult<Self> {
        let mut url = Url::parse(input)
            .map_err(|e| InvalidURL::new_err(format!("invalid base_url {input:?}: {e}")))?;
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        Ok(Self(url))
    }

    pub fn to_py(&self) -> PyURL {
        PyURL::new(UrlReference::from_url(self.0.clone()))
    }
}

/// A URL argument: `str` or `rqx.URL`, resolved against the client's base and
/// checked for a scheme rqx can send.
pub struct RequestUrl(String);

impl RequestUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn resolve(&self, base: Option<&BaseUrl>, params: Option<QueryPairs>) -> PyResult<Url> {
        let absolute = match Url::parse(&self.0) {
            Ok(url) if url.has_authority() => Some(url),
            Ok(_) | Err(ParseError::RelativeUrlWithoutBase) => None,
            Err(e) => {
                return Err(InvalidURL::new_err(format!(
                    "invalid URL {:?}: {e}",
                    self.0
                )));
            }
        };

        let mut url = match (absolute, base) {
            (Some(url), _) => url,
            (None, Some(base)) => {
                // A reference contributes its path and query only. An authority
                // it carries (`//other.example/x`) is not a host rqx will
                // target, which is how httpx merges it too.
                let path = UrlReference::parse(&self.0)?.raw_path();
                base.0.join(path.trim_start_matches('/')).map_err(|e| {
                    InvalidURL::new_err(format!("could not join base_url with {:?}: {e}", self.0))
                })?
            }
            (None, None) => {
                return Err(UnsupportedProtocol::new_err(
                    "Request URL is missing an 'http://' or 'https://' protocol.",
                ));
            }
        };

        // `params=` replaces whatever query the URL carried, as in httpx.
        if let Some(params) = params {
            let query = params.to_string();
            url.set_query(Some(query.as_str()).filter(|q| !q.is_empty()));
        }

        match url.scheme() {
            "http" | "https" => Ok(url),
            scheme => Err(UnsupportedProtocol::new_err(format!(
                "Request URL has an unsupported protocol '{scheme}://'."
            ))),
        }
    }
}

impl<'py> FromPyObject<'_, 'py> for RequestUrl {
    type Error = PyErr;

    fn extract(obj: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        let obj = obj.to_owned();
        if let Ok(s) = obj.cast::<PyString>() {
            return Ok(Self(s.to_cow()?.into_owned()));
        }
        Ok(Self(PyURL::extract_reference(&obj)?.to_string()))
    }
}
