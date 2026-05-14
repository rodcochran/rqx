//! URL helpers.
//!
//! Currently exposes two functions used by `PyClient` / `PyAsyncClient` to
//! support the `base_url=` parameter. This module is also where a future
//! `PyURL` type (httpx.URL parity) will live — see Issue #59.

use pyo3::PyResult;
use pyo3::exceptions::PyValueError;
use url::Url;

/// Parse a `base_url=` argument into a canonicalized URL.
///
/// We force a trailing `/` on the path so the standard RFC 3986 join
/// behavior gives users what they intuitively expect: `base_url + "/users"`
/// resolves to `<base>/users` rather than dropping the last path segment.
/// This mirrors how httpx normalizes its base_url at construction time.
pub fn parse_base_url(s: &str) -> PyResult<Url> {
    let mut url = Url::parse(s)
        .map_err(|e| PyValueError::new_err(format!("invalid base_url {s:?}: {e}")))?;
    if !url.path().ends_with('/') {
        let new_path = format!("{}/", url.path());
        url.set_path(&new_path);
    }
    Ok(url)
}

/// Resolve a per-request URL against an optional client base URL.
///
/// Rules (chosen to match httpx's `_merge_url`):
///   - If `input` is already an absolute URL (has a scheme + host), use it
///     as-is — the base is ignored.
///   - Otherwise, if `base` is set: strip a leading `/` from `input`, then
///     join. Combined with the trailing-`/` canonicalization in
///     `parse_base_url`, this preserves the base's path segments instead of
///     dropping them per strict RFC 3986 resolution.
///   - If there's no base and `input` isn't absolute, pass through and let
///     reqwest raise the URL parse error as it does today.
pub fn resolve_url(base: Option<&Url>, input: &str) -> PyResult<String> {
    // Cheap absolute-URL check: if parsing as a full URL succeeds and yields
    // a non-empty host, it's absolute and overrides any base.
    if let Ok(parsed) = Url::parse(input) {
        if parsed.has_host() {
            return Ok(input.to_string());
        }
    }
    match base {
        None => Ok(input.to_string()),
        Some(b) => {
            let stripped = input.trim_start_matches('/');
            b.join(stripped)
                .map(|u| u.to_string())
                .map_err(|e| {
                    PyValueError::new_err(format!(
                        "could not join base_url with {input:?}: {e}"
                    ))
                })
        }
    }
}
