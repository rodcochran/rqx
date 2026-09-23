use pyo3::prelude::{PyResult, pyclass, pymethods};
use std::collections::HashSet;

use rqx_core::retry::Retry;

/// `total` counts retries, not attempts: total=3 allows four attempts. Under
/// follow_redirects the caps apply per hop; num_retries and retry_history on
/// the final response add up across the chain.
#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct PyRetry {
    pub(crate) inner: Retry,
}

impl PyRetry {
    pub fn new(inner: Retry) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyRetry {
    #[new]
    #[pyo3(signature = (
        total=None,
        connect=None,
        read=None,
        status=None,
        backoff_factor=None,
        backoff_max=None,
        backoff_jitter=None,
        status_forcelist=None,
        allowed_methods=None,
        respect_retry_after_header=None,
        raise_on_status=None,
        raise_on_redirect=None,
        total_timeout=None,
    ))]
    fn __new__(
        total: Option<i32>,
        connect: Option<i32>,
        read: Option<i32>,
        status: Option<i32>,
        backoff_factor: Option<f32>,
        backoff_max: Option<f32>,
        backoff_jitter: Option<f32>,
        status_forcelist: Option<HashSet<u16>>,
        allowed_methods: Option<HashSet<String>>,
        respect_retry_after_header: Option<bool>,
        raise_on_status: Option<bool>,
        raise_on_redirect: Option<bool>,
        total_timeout: Option<f64>,
    ) -> PyResult<Self> {
        Ok(Self {
            inner: Retry::new(
                total,
                connect,
                read,
                status,
                backoff_factor,
                backoff_max,
                backoff_jitter,
                status_forcelist,
                allowed_methods,
                respect_retry_after_header,
                raise_on_status,
                raise_on_redirect,
                total_timeout,
            ),
        })
    }

    // maximum total retry attempts (across all failure modes)
    #[getter]
    pub fn total(&self) -> i32 {
        self.inner.total
    }

    // max retries on connection errors (defaults to total)
    #[getter]
    pub fn connect(&self) -> i32 {
        self.inner.connect
    }

    // max retries on read errors (defaults to total)
    #[getter]
    pub fn read(&self) -> i32 {
        self.inner.read
    }

    // max retries on bad status codes (defaults to total)
    #[getter]
    pub fn status(&self) -> i32 {
        self.inner.status
    }

    // multiplier for exponential backoff between retries
    #[getter]
    pub fn backoff_factor(&self) -> f32 {
        self.inner.backoff_factor
    }

    // ceiling on computed backoff delay in seconds
    #[getter]
    pub fn backoff_max(&self) -> f32 {
        self.inner.backoff_max
    }

    // random jitter added to backoff (0.0 = no jitter)
    #[getter]
    pub fn backoff_jitter(&self) -> f32 {
        self.inner.backoff_jitter
    }

    // set of status codes that trigger a retry
    #[getter]
    pub fn status_forcelist(&self) -> HashSet<u16> {
        self.inner.status_forcelist.clone()
    }

    // only retry requests with these HTTP methods
    #[getter]
    pub fn allowed_methods(&self) -> HashSet<String> {
        self.inner.allowed_methods.clone()
    }

    // honor Retry-After header delay when present
    #[getter]
    pub fn respect_retry_after_header(&self) -> bool {
        self.inner.respect_retry_after_header
    }

    // raise MaxRetriesExceeded when retries exhausted
    #[getter]
    pub fn raise_on_status(&self) -> bool {
        self.inner.raise_on_status
    }

    // raise TooManyRedirects when redirect loop detected
    #[getter]
    pub fn raise_on_redirect(&self) -> bool {
        self.inner.raise_on_redirect
    }

    // raise MaxRetriesExceeded when total time in retry exceeds max
    #[getter]
    pub fn total_timeout(&self) -> Option<f64> {
        self.inner.total_timeout
    }
}
