use pyo3::prelude::PyResult;

use crate::exceptions::*;

/// Pre-validated HTTP version selection.
///
///   - `Negotiate`: ALPN-driven (both args None, or both True). h2 preferred,
///     h1.1 fallback over TLS; h1.1 for plain HTTP.
///   - `Http1Only`: `http1=True, http2=False` — never upgrade.
///   - `Http2Only`: `http1=False, http2=True` — prior knowledge, no fallback.
///
/// The `(false, false)` combination is rejected at parse time so the builder
/// method itself is infallible.
pub enum HttpVersionConfig {
    Negotiate,
    Http1Only,
    Http2Only,
}

impl HttpVersionConfig {
    pub fn from_args(http1: Option<bool>, http2: Option<bool>) -> PyResult<Self> {
        let allow_h1 = http1.unwrap_or(true);
        let allow_h2 = http2.unwrap_or(true);
        match (allow_h1, allow_h2) {
            (true, true) => Ok(Self::Negotiate),
            (true, false) => Ok(Self::Http1Only),
            (false, true) => Ok(Self::Http2Only),
            (false, false) => Err(RqxError::new_err(
                "at least one of http1, http2 must be true",
            )),
        }
    }
}
