use pyo3::Bound;
use pyo3::prelude::{PyRef, PyResult, pyclass, pymethods};
use pyo3::types::PyAny;
use reqwest::tls::Identity;
use reqwest::{Client, ClientBuilder, Request, Response};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::exceptions::*;
use crate::http::protocol::HttpVersionConfig;
use crate::http::tls::{VerifyConfig, parse_identity};
use crate::request::RequestSpec;
use crate::response::PendingResponse;
use crate::retry::{FailureKind, PyRetry, RetryCounts};
use crate::timeout::PyTimeout;

use rqx_core::http::proxy::ProxyParser;
use rqx_core::transport::{RqxClientBuilder, Transport};

/// Orchestrates the full builder chain from Python-flavored config args
/// into a reqwest `Client`. All pyo3 parsing happens here at the top; the
/// builder methods themselves take plain Rust types.
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
    let (connect_timeout, read_timeout, pool_timeout) = match timeout {
        Some(t) => {
            let parsed = PyTimeout::extract_any(t)?;
            (parsed.connect, parsed.read, parsed.pool)
        }
        None => (None, None, None),
    };

    let verify_cfg = verify.map(VerifyConfig::from_py_any).transpose()?;
    let identity = cert.map(parse_identity).transpose()?;
    let http_version = HttpVersionConfig::from_args(http1, http2)?;
    let proxies = ProxyParser::from_hash_map(proxy)?;

    let client = RqxClientBuilder::new()
        .with_pool(max_keepalive_connections, keepalive_expiry, pool_timeout)
        .with_http_version(http_version)
        .with_phase_timeouts(connect_timeout, read_timeout)
        .with_proxy(proxies)
        .with_tls(verify_cfg, identity)
        .build();

    Ok(client)
}

// ────────────────────────────────────────────────────────────────────────
// HTTPTransport — synchronous Python-facing transport
// ────────────────────────────────────────────────────────────────────────

#[pyclass(skip_from_py_object)]
#[derive(Clone, Default)]
pub struct HTTPTransport {
    pub(crate) inner: Transport,
}

#[pymethods]
impl HTTPTransport {
    #[new]
    #[pyo3(signature = (
        retries=None,
        max_connections=None,
        max_keepalive_connections=None,
        keepalive_expiry=None,
        http1=None,
        http2=None,
        verify=None,
        cert=None,
        proxy=None,
        timeout=None,
    ))]
    fn __new__(
        retries: Option<PyRef<'_, PyRetry>>,
        max_connections: Option<u32>,
        max_keepalive_connections: Option<u32>,
        keepalive_expiry: Option<f64>,
        http1: Option<bool>,
        http2: Option<bool>,
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        proxy: Option<HashMap<String, String>>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let retries = retries.map(|r| r.clone());
        let http_client = build_http_client(
            max_keepalive_connections,
            keepalive_expiry,
            http1,
            http2,
            verify,
            cert,
            proxy,
            timeout,
        )?;
        let semaphore = max_connections.map(|mc| Arc::new(Semaphore::new(mc as usize)));
        Ok(Self {
            inner: Transport::new(http_client, semaphore, retries),
        })
    }

    #[getter]
    fn retries(&self) -> Option<PyRetry> {
        self.inner.retries.clone()
    }
}

impl HTTPTransport {
    pub fn new(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        if verify.is_none() && cert.is_none() && timeout.is_none() {
            return Ok(HTTPTransport::default());
        }
        let client = build_http_client(None, None, None, None, verify, cert, None, timeout)?;
        Ok(Self {
            inner: Transport::new(client, None, None),
        })
    }

    pub fn client(&self) -> &Client {
        self.inner.client()
    }
}

// ────────────────────────────────────────────────────────────────────────
// AsyncHTTPTransport — async Python-facing transport
// ────────────────────────────────────────────────────────────────────────

#[pyclass(skip_from_py_object)]
#[derive(Clone, Default)]
pub struct AsyncHTTPTransport {
    pub(crate) inner: Transport,
}

#[pymethods]
impl AsyncHTTPTransport {
    #[new]
    #[pyo3(signature = (
        retries=None,
        max_connections=None,
        max_keepalive_connections=None,
        keepalive_expiry=None,
        http1=None,
        http2=None,
        verify=None,
        cert=None,
        proxy=None,
        timeout=None,
    ))]
    fn __new__(
        retries: Option<PyRef<'_, PyRetry>>,
        max_connections: Option<u32>,
        max_keepalive_connections: Option<u32>,
        keepalive_expiry: Option<f64>,
        http1: Option<bool>,
        http2: Option<bool>,
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        proxy: Option<HashMap<String, String>>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let retries = retries.map(|r| r.clone());
        let http_client = build_http_client(
            max_keepalive_connections,
            keepalive_expiry,
            http1,
            http2,
            verify,
            cert,
            proxy,
            timeout,
        )?;
        let semaphore = max_connections.map(|mc| Arc::new(Semaphore::new(mc as usize)));
        Ok(Self {
            inner: Transport::new(http_client, semaphore, retries),
        })
    }

    #[getter]
    fn retries(&self) -> Option<PyRetry> {
        self.inner.retries.clone()
    }
}

impl AsyncHTTPTransport {
    pub fn new(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        if verify.is_none() && cert.is_none() && timeout.is_none() {
            return Ok(AsyncHTTPTransport::default());
        }
        let client = build_http_client(None, None, None, None, verify, cert, None, timeout)?;
        Ok(Self {
            inner: Transport::new(client, None, None),
        })
    }

    pub fn client(&self) -> &Client {
        self.inner.client()
    }
}
