use std::collections::HashMap;

use pyo3::Bound;
use pyo3::prelude::{PyRef, pyclass, pymethods};
use pyo3::types::PyAny;

use rqx_core::timeout::Timeout;
use rqx_core::transport::{Transport, TransportConfig};

use crate::config_builders::transport::TransportArgs;
use crate::exceptions::PyRqxError;
use crate::retry::PyRetry;
use crate::timeout::TimeoutArg;

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
            max_connections: max_connections,
            max_keepalive_connections: max_keepalive_connections,
            keepalive_expiry: keepalive_expiry,
            http1: http1,
            http2: http2,
            verify,
            cert,
            proxy: proxy,
            timeout: timeout.map(Timeout::from),
            retries: retries,
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
            max_connections: max_connections,
            max_keepalive_connections: max_keepalive_connections,
            keepalive_expiry: keepalive_expiry,
            http1: http1,
            http2: http2,
            verify,
            cert,
            proxy: proxy,
            timeout: timeout.map(Timeout::from),
            retries: retries,
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
