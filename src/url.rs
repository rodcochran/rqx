//! URL helpers.
//!
//! Currently exposes two functions used by `PyClient` / `PyAsyncClient` to
//! support the `base_url=` parameter. This module is also where a future
//! `PyURL` type (httpx.URL parity) will live — see Issue #59.

use pyo3::PyResult;
use pyo3::exceptions::PyValueError;
use url::{ParseError, Url};

use crate::exceptions::UnsupportedProtocol;

/// Parse a `base_url=` argument into a canonicalized URL.
///
/// We force a trailing `/` on the path so the standard RFC 3986 join
/// behavior gives users what they intuitively expect: `base_url + "/users"`
/// resolves to `<base>/users` rather than dropping the last path segment.
/// This mirrors how httpx normalizes its base_url at construction time.
pub fn parse_base_url(s: &str) -> PyResult<Url> {
    let mut url =
        Url::parse(s).map_err(|e| PyValueError::new_err(format!("invalid base_url {s:?}: {e}")))?;
    if !url.path().ends_with('/') {
        let new_path = format!("{}/", url.path());
        url.set_path(&new_path);
    }
    Ok(url)
}

/// Resolve a per-request URL against an optional client base URL.
///
/// An absolute URL is used as-is; anything else is joined onto the base
/// (leading `/` stripped so the base's path segments survive). The result
/// must be http or https.
pub fn resolve_url(base: Option<&Url>, input: &str) -> PyResult<Url> {
    let absolute = match Url::parse(input) {
        Ok(url) if url.has_authority() => Some(url),
        Ok(_) | Err(ParseError::RelativeUrlWithoutBase) => None,
        Err(e) => return Err(PyValueError::new_err(format!("invalid URL {input:?}: {e}"))),
    };

    let url = match (absolute, base) {
        (Some(url), _) => url,
        (None, Some(base)) => base.join(input.trim_start_matches('/')).map_err(|e| {
            PyValueError::new_err(format!("could not join base_url with {input:?}: {e}"))
        })?,
        (None, None) => {
            return Err(UnsupportedProtocol::new_err(
                "Request URL is missing an 'http://' or 'https://' protocol.",
            ));
        }
    };

    match url.scheme() {
        "http" | "https" => Ok(url),
        scheme => Err(UnsupportedProtocol::new_err(format!(
            "Request URL has an unsupported protocol '{scheme}://'."
        ))),
    }
}
