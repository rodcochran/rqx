use std::collections::HashMap;

use pyo3::Bound;
use pyo3::prelude::{PyRef, pyclass, pymethods};
use pyo3::types::PyAny;

use rqx_core::http::protocol::HttpVersionConfig;
use rqx_core::http::proxy::ProxyParser;
use rqx_core::http::tls::VerifyConfig;
use rqx_core::timeout::Timeout;
use rqx_core::transport::{ConnectionPoolConfig, Transport, TransportConfig};

use crate::exceptions::PyRqxError;
use crate::http::tls::{parse_identity, parse_verify_config_from_py};
use crate::retry::PyRetry;
use crate::timeout::TimeoutArg;

#[derive(Default)]
pub struct TransportArgs<'py> {
    pub max_connections: Option<u32>,
    pub max_keepalive_connections: Option<u32>,
    pub keepalive_expiry: Option<f64>,
    pub http1: Option<bool>,
    pub http2: Option<bool>,
    pub verify: Option<&'py Bound<'py, PyAny>>,
    pub cert: Option<&'py Bound<'py, PyAny>>,
    pub proxy: Option<HashMap<String, String>>,
    pub timeout: Option<Timeout>,
    pub retries: Option<PyRef<'py, PyRetry>>,
}

impl TryFrom<TransportArgs<'_>> for TransportConfig {
    type Error = PyRqxError;
    fn try_from(args: TransportArgs<'_>) -> Result<Self, Self::Error> {
        let timeout = args.timeout.unwrap_or_default();

        let verify_config = args
            .verify
            .map(parse_verify_config_from_py)
            .transpose()?
            .unwrap_or(VerifyConfig::Default);
        let cert = args.cert.map(parse_identity).transpose()?;
        let http_version = HttpVersionConfig::from_args(args.http1, args.http2)?;
        let proxies = ProxyParser::from_hash_map(args.proxy)?;

        let pool_config = ConnectionPoolConfig::new(
            args.max_connections,
            args.max_keepalive_connections,
            args.keepalive_expiry,
        );

        let retry_config = args.retries.map(|r| r.inner.clone());

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
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<Self, PyRqxError> {
        let transport_config = TransportConfig::try_from(TransportArgs {
            max_connections,
            max_keepalive_connections,
            keepalive_expiry,
            http1,
            http2,
            verify,
            cert,
            proxy,
            timeout: timeout.map(Timeout::from),
            retries,
        })?;
        Ok(Self {
            inner: Transport::new(transport_config)?,
        })
    }

    #[getter]
    fn retries(&self) -> Option<PyRetry> {
        self.inner.config.retry_config.clone().map(PyRetry::new)
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
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<Self, PyRqxError> {
        let transport_config = TransportConfig::try_from(TransportArgs {
            max_connections,
            max_keepalive_connections,
            keepalive_expiry,
            http1,
            http2,
            verify,
            cert,
            proxy,
            timeout: timeout.map(Timeout::from),
            retries,
        })?;
        Ok(Self {
            inner: Transport::new(transport_config)?,
        })
    }

    #[getter]
    fn retries(&self) -> Option<PyRetry> {
        self.inner.config.retry_config.clone().map(PyRetry::new)
    }
}
