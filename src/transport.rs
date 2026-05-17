use pyo3::Bound;
use pyo3::prelude::{PyRef, PyResult, Python, pyclass, pymethods};
use pyo3::types::PyAny;
use reqwest::{Client, Request, Response};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

use super::exceptions::*;
use super::response::PyResponse;
use super::retry::PyRetry;
use super::runtime::RUNTIME;

use super::http::client_builder::build_http_client;
use super::http::transport::Transport;

#[pyclass(skip_from_py_object)]
#[derive(Clone, Default)]
pub struct HTTPTransport {
    pub(crate) inner: Transport,
}

#[pyclass(skip_from_py_object)]
#[derive(Clone, Default)]
pub struct AsyncHTTPTransport {
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
        let _retries = retries.map(|r| r.clone());

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

        let max_connection_semaphore =
            max_connections.map(|mc| Arc::new(Semaphore::new(mc as usize)));

        Ok(Self {
            inner: Transport::new(http_client, max_connection_semaphore, _retries),
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
}

impl HTTPTransport {
    pub fn handle_request(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        let response = py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| RqxError::new_err("runtime not initialized"))?
                // NOTE: block_on panics if called from within an existing tokio runtime
                // context ("Cannot start a runtime from within a runtime"). Safe here
                // because Python is the caller and py.detach releases the GIL without
                // entering a runtime. Callers embedding this in an async Python framework
                // (or invoking it from inside another tokio task) will panic — they should
                // use the async variant of this API instead.
                .block_on(async { self.inner.handle_request(request).await })
        })?;
        return Ok(response);
    }

    pub fn send_raw(&self, py: Python<'_>, request: Request) -> PyResult<Response> {
        py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| RqxError::new_err("runtime not initialized"))?
                .block_on(self.inner.send_raw(request))
        })
    }

    pub fn client(&self) -> &Client {
        self.inner.client()
    }
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
        let _retries = retries.map(|r| r.clone());

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

        let max_connection_semaphore =
            max_connections.map(|mc| Arc::new(Semaphore::new(mc as usize)));

        Ok(Self {
            inner: Transport::new(http_client, max_connection_semaphore, _retries),
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
}

impl AsyncHTTPTransport {
    pub async fn handle_request(&self, request: Request) -> PyResult<PyResponse> {
        let response = self.inner.handle_request(request).await?;
        return Ok(response);
    }

    pub async fn send_raw(&self, request: Request) -> PyResult<Response> {
        self.inner.send_raw(request).await
    }

    pub fn client(&self) -> &Client {
        self.inner.client()
    }
}
