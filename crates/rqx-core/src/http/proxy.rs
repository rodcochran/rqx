use std::collections::HashMap;

use crate::error::{RqxError, TransportError};

pub fn parse_proxies(
    proxy: Option<HashMap<String, String>>,
) -> Result<Vec<reqwest::Proxy>, RqxError> {
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
        .map_err(|e| TransportError::ProxyError(format!("invalid proxy: {e}")))?;
        out.push(p);
    }
    Ok(out)
}
