use std::collections::HashMap;

use pyo3::Bound;
use pyo3::prelude::PyRef;
use pyo3::types::PyAny;
use rqx_core::http::tls::VerifyConfig;
use rqx_core::timeout::Timeout;

use crate::exceptions::PyRqxError;
use crate::http::tls::{parse_identity, parse_verify_config_from_py};
use crate::retry::PyRetry;

use rqx_core::http::protocol::HttpVersionConfig;
use rqx_core::http::proxy::ProxyParser;
use rqx_core::transport::{ConnectionPoolConfig, TransportConfig};

pub fn build_transport_config(
    max_connections: Option<u32>,
    max_keepalive_connections: Option<u32>,
    keepalive_expiry: Option<f64>,
    http1: Option<bool>,
    http2: Option<bool>,
    verify: Option<&Bound<'_, PyAny>>,
    cert: Option<&Bound<'_, PyAny>>,
    proxy: Option<HashMap<String, String>>,
    timeout: Option<Timeout>,
    retries: Option<PyRef<'_, PyRetry>>,
) -> Result<TransportConfig, PyRqxError> {
    let timeout = timeout.unwrap_or_default();

    let verify_config = verify
        .map(parse_verify_config_from_py)
        .transpose()?
        .unwrap_or(VerifyConfig::Default);
    let cert = cert.map(parse_identity).transpose()?;
    let http_version = HttpVersionConfig::from_args(http1, http2)?;
    let proxies = ProxyParser::from_hash_map(proxy)?;

    let pool_config = ConnectionPoolConfig::new(
        max_connections,
        max_keepalive_connections,
        keepalive_expiry,
        timeout.pool,
    );

    let retry_config = retries.map(|r| r.inner.clone());

    Ok(TransportConfig::new(
        pool_config,
        http_version,
        verify_config,
        timeout,
        retry_config,
        cert,
        proxies,
    ))
}
