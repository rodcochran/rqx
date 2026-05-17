use pyo3::Bound;
use pyo3::prelude::PyResult;
use pyo3::types::{PyAny, PyAnyMethods, PyBool, PyBytes, PyString, PyTuple};
use reqwest::Client;
use reqwest::tls::{Certificate, Identity};
use std::collections::HashMap;
use std::time::Duration;

use crate::exceptions::*;
use crate::timeout::PyTimeout;
/*

Helper for constructing the HTTP Client

*/
pub fn build_http_client(
    max_keepalive_connections: Option<u32>,
    keepalive_expiry: Option<f64>,
    http1: Option<bool>,
    http2: Option<bool>,
    verify: Option<&Bound<'_, PyAny>>,
    cert: Option<&Bound<'_, PyAny>>,
    proxy: Option<HashMap<String, String>>,
    timeout: Option<&Bound<'_, PyAny>>,
) -> PyResult<Client> {
    let mut http_client_builder = Client::builder()
        // Explicitly add no redirects at the transport level, as we let the PyClient take care of it
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true);

    if let Some(max_keepalive) = max_keepalive_connections {
        http_client_builder = http_client_builder.pool_max_idle_per_host(max_keepalive as usize);
    }

    if let Some(ke) = keepalive_expiry {
        http_client_builder = http_client_builder.pool_idle_timeout(Duration::from_secs_f64(ke));
    }

    // Phase timeouts. See PyTimeout for semantics. write= is currently a no-op
    // because reqwest doesn't expose a per-phase write timeout. pool= maps to
    // pool_idle_timeout, which is close to but not exactly httpx's
    // pool-acquisition timeout.
    if let Some(t) = timeout {
        let parsed = PyTimeout::extract_any(t)?;
        if let Some(c) = parsed.connect {
            http_client_builder = http_client_builder.connect_timeout(Duration::from_secs_f64(c));
        }
        if let Some(r) = parsed.read {
            http_client_builder = http_client_builder.read_timeout(Duration::from_secs_f64(r));
        }
        if let Some(p) = parsed.pool {
            // Only set if keepalive_expiry didn't already.
            if keepalive_expiry.is_none() {
                http_client_builder =
                    http_client_builder.pool_idle_timeout(Duration::from_secs_f64(p));
            }
        }
    }

    // HTTP version selection:
    //   - default (both None / both True): ALPN negotiates over TLS (h2 preferred,
    //     h1.1 fallback). For plain HTTP, reqwest uses h1.1.
    //   - http1=True, http2=False: HTTP/1.1 only — never upgrade.
    //   - http1=False, http2=True: HTTP/2 prior knowledge — no fallback. Will
    //     fail against h1-only servers; use when you know the server speaks h2.
    //   - both False: error — at least one protocol must be allowed.
    let allow_h1 = http1.unwrap_or(true);
    let allow_h2 = http2.unwrap_or(true);
    match (allow_h1, allow_h2) {
        (false, false) => {
            return Err(RqxError::new_err(
                "at least one of http1, http2 must be true",
            ));
        }
        (true, false) => {
            http_client_builder = http_client_builder.http1_only();
        }
        (false, true) => {
            http_client_builder = http_client_builder.http2_prior_knowledge();
        }
        (true, true) => {
            // No-op — reqwest's default does ALPN negotiation over TLS.
        }
    }

    if let Some(v) = verify {
        if v.is_instance_of::<PyBool>() {
            let verify_enabled = v.extract::<bool>().unwrap();
            if !verify_enabled {
                http_client_builder = http_client_builder.danger_accept_invalid_certs(true);
            }
        } else if v.is_instance_of::<PyString>() {
            let path = v
                .extract::<String>()
                .map_err(|e| RqxError::new_err(format!("failed to parse CA cert path: {e}")))?;
            let bytes = std::fs::read(&path)
                .map_err(|e| RqxError::new_err(format!("failed to read CA cert: {e}")))?;
            let cert = Certificate::from_pem(&bytes)
                .map_err(|e| RqxError::new_err(format!("failed to construct CA cert: {e}")))?;

            http_client_builder = http_client_builder.add_root_certificate(cert);
        }
    }

    if let Some(c) = cert {
        if c.is_instance_of::<PyString>() {
            let path = c
                .extract::<String>()
                .map_err(|e| RqxError::new_err(format!("failed to parse client cert path: {e}")))?;
            let bytes = std::fs::read(&path)
                .map_err(|e| RqxError::new_err(format!("failed to read client cert: {e}")))?;
            let identity = Identity::from_pem(&bytes)
                .map_err(|e| RqxError::new_err(format!("failed to construct client cert: {e}")))?;
            http_client_builder = http_client_builder.identity(identity);
        } else if c.is_instance_of::<PyBytes>() {
            let bytes: Vec<u8> = c
                .extract()
                .map_err(|e| RqxError::new_err(format!("failed to read cert bytes: {e}")))?;
            let identity = Identity::from_pem(&bytes)
                .map_err(|e| RqxError::new_err(format!("failed to parse client cert: {e}")))?;
            http_client_builder = http_client_builder.identity(identity);
        } else if c.is_instance_of::<PyTuple>() {
            let tup: (String, String) = c
                .extract()
                .map_err(|e| RqxError::new_err(format!("failed to parse cert, key tuple: {e}")))?;
            let cert_path: String = tup.0;
            let key_path: String = tup.1;
            let mut bytes = std::fs::read(&cert_path)
                .map_err(|e| RqxError::new_err(format!("failed to read {cert_path}: {e}")))?;
            let mut key_bytes = std::fs::read(&key_path)
                .map_err(|e| RqxError::new_err(format!("failed to read {key_path}: {e}")))?;
            bytes.append(&mut key_bytes);
            let identity = Identity::from_pem(&bytes)
                .map_err(|e| RqxError::new_err(format!("failed to parse client cert+key: {e}")))?;
            http_client_builder = http_client_builder.identity(identity);
        } else {
            return Err(RqxError::new_err(
                "cert must be str (path), tuple(str, str) of (cert, key) paths, or bytes (PEM)",
            ));
        }
    }

    if let Some(proxies) = proxy {
        for (scheme, url) in proxies {
            let p = match scheme.as_str() {
                "http" => reqwest::Proxy::http(&url),
                "https" => reqwest::Proxy::https(&url),
                _ => continue,
            }
            .map_err(|e| RqxError::new_err(format!("invalid proxy: {e}")))?;
            http_client_builder = http_client_builder.proxy(p);
        }
    }

    let http_client = http_client_builder
        .build()
        .expect("Failed to build HTTP client");

    return Ok(http_client);
}
