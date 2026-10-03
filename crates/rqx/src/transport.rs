use pyo3::Bound;
use pyo3::prelude::{PyRef, pyclass, pymethods};
use pyo3::types::PyAny;

use std::collections::HashMap;

use crate::config_builders::transport::build_transport_config;
use crate::exceptions::PyRqxError;
use crate::retry::PyRetry;
use crate::timeout::TimeoutArg;

use rqx_core::timeout::Timeout;
use rqx_core::transport::Transport;

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
        let transport_config = build_transport_config(
            max_connections,
            max_keepalive_connections,
            keepalive_expiry,
            http1,
            http2,
            verify,
            cert,
            proxy,
            timeout.map(Timeout::from),
            retries,
        )?;
        Ok(Self {
            inner: Transport::new(transport_config)?,
        })
    }

    #[getter]
    fn retries(&self) -> Option<PyRetry> {
        self.inner.config.retry_config.clone().map(PyRetry::new)
    }
}

impl HTTPTransport {
    pub fn new(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<Timeout>,
    ) -> Result<Self, PyRqxError> {
        if verify.is_none() && cert.is_none() && timeout.is_none() {
            return Ok(HTTPTransport::default());
        }
        let transport_config = build_transport_config(
            None, None, None, None, None, verify, cert, None, timeout, None,
        )?;

        Ok(Self {
            inner: Transport::new(transport_config)?,
        })
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
        let transport_config = build_transport_config(
            max_connections,
            max_keepalive_connections,
            keepalive_expiry,
            http1,
            http2,
            verify,
            cert,
            proxy,
            timeout.map(Timeout::from),
            retries,
        )?;
        Ok(Self {
            inner: Transport::new(transport_config)?,
        })
    }

    #[getter]
    fn retries(&self) -> Option<PyRetry> {
        self.inner.config.retry_config.clone().map(PyRetry::new)
    }
}

impl AsyncHTTPTransport {
    pub fn new(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<Timeout>,
    ) -> Result<Self, PyRqxError> {
        if verify.is_none() && cert.is_none() && timeout.is_none() {
            return Ok(AsyncHTTPTransport::default());
        }
        let transport_config = build_transport_config(
            None, None, None, None, None, verify, cert, None, timeout, None,
        )?;

        Ok(Self {
            inner: Transport::new(transport_config)?,
        })
    }
}
