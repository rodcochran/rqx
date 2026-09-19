use pyo3::prelude::PyResult;
use std::collections::HashMap;

use crate::exceptions::*;

/// Parses the Python `proxy` dict into `reqwest::Proxy` values.
/// Map keys: "http" | "https" (others silently ignored).
pub fn parse_proxies(proxy: Option<HashMap<String, String>>) -> PyResult<Vec<reqwest::Proxy>> {
    let Some(map) = proxy else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(map.len());
    for (scheme, url) in map {
        let p = match scheme.as_str() {
            "http" => reqwest::Proxy::http(&url),
            "https" => reqwest::Proxy::https(&url),
            _ => continue,
        }
        .map_err(|e| RqxError::new_err(format!("invalid proxy: {e}")))?;
        out.push(p);
    }
    Ok(out)
}
