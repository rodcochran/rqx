use std::collections::HashSet;
use pyo3::prelude::{PyResult, pyclass, pymethods};

const DEFAULT_TOTAL_RETRIES: i32 = 3;
const DEFAULT_BACKOFF_FACTOR: f32 = 0.0;
const DEFAULT_BACKOFF_MAX: f32 = 120.0;
const DEFAULT_BACKOFF_JITTER: f32 = 0.0;
const DEFAULT_STATUS_FORCELIST: &[u16] = &[];
const DEFAULT_ALLOWED_METHODS: &[&str] = &[
    "DELETE", 
    "GET", 
    "HEAD", 
    "OPTIONS", 
    "PUT", 
    // "TRACE"
];
const DEFAULT_RESPECT_RETRY_AFTER_HEADER: bool = true;
const DEFAULT_RAISE_ON_STATUS: bool = true;
pub(crate) const DEFAULT_RAISE_ON_REDIRECT: bool = true;
const DEFAULT_TOTAL_TIMEOUT: Option<f64> = None;


#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct PyRetry {
    // maximum total retry attempts (across all failure modes)
    #[pyo3(get)]
    pub total: i32,

    // max retries on connection errors (defaults to total)
    #[pyo3(get)]
    pub connect: i32,

    // max retries on read errors (defaults to total)
    #[pyo3(get)]
    pub read: i32,

    // max retries on bad status codes (defaults to total)
    #[pyo3(get)]
    pub status: i32,

    // multiplier for exponential backoff between retries
    #[pyo3(get)]
    pub backoff_factor: f32,

    // ceiling on computed backoff delay in seconds
    #[pyo3(get)]
    pub backoff_max: f32,
    
    // random jitter added to backoff (0.0 = no jitter)
    #[pyo3(get)]
    pub backoff_jitter: f32,

    // set of status codes that trigger a retry
    #[pyo3(get)]
    pub status_forcelist: HashSet<u16>,

    // only retry requests with these HTTP methods
    #[pyo3(get)]
    pub allowed_methods: HashSet<String>,

    // honor Retry-After header delay when present
    #[pyo3(get)]
    pub respect_retry_after_header: bool,

    // raise MaxRetriesExceeded when retries exhausted
    #[pyo3(get)]
    pub raise_on_status: bool,

    // raise TooManyRedirects when redirect loop detected
    #[pyo3(get)]
    pub raise_on_redirect: bool,

    // raise MaxRetriesExceeded when total time in retry exceeds max
    #[pyo3(get)]
    pub total_timeout: Option<f64>,
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

        let default_total = total.unwrap_or(DEFAULT_TOTAL_RETRIES);

        Ok(
            Self {
                total: default_total,
                connect: connect.unwrap_or(default_total),
                read: read.unwrap_or(default_total),
                status: status.unwrap_or(default_total),
                backoff_factor: backoff_factor.unwrap_or(DEFAULT_BACKOFF_FACTOR),
                backoff_max: backoff_max.unwrap_or(DEFAULT_BACKOFF_MAX),
                backoff_jitter: backoff_jitter.unwrap_or(DEFAULT_BACKOFF_JITTER),
                status_forcelist: status_forcelist.unwrap_or(
                    DEFAULT_STATUS_FORCELIST
                        .iter()
                        .copied()
                        .collect()
                ),
                allowed_methods: allowed_methods.unwrap_or(
                    DEFAULT_ALLOWED_METHODS
                        .iter()
                        .map(ToString::to_string)
                        .collect()
                ),
                respect_retry_after_header: respect_retry_after_header.unwrap_or(
                    DEFAULT_RESPECT_RETRY_AFTER_HEADER
                ),
                raise_on_status: raise_on_status.unwrap_or(DEFAULT_RAISE_ON_STATUS),
                raise_on_redirect: raise_on_redirect.unwrap_or(DEFAULT_RAISE_ON_REDIRECT),
                total_timeout: total_timeout
            }
        )

    }
}

impl PyRetry {
    pub fn with_defaults() -> Self {
        Self {
            total: DEFAULT_TOTAL_RETRIES,
            connect: DEFAULT_TOTAL_RETRIES,
            read: DEFAULT_TOTAL_RETRIES,
            status: DEFAULT_TOTAL_RETRIES,
            backoff_factor: DEFAULT_BACKOFF_FACTOR,
            backoff_max: DEFAULT_BACKOFF_MAX,
            backoff_jitter: DEFAULT_BACKOFF_JITTER,
            status_forcelist: DEFAULT_STATUS_FORCELIST
                .iter()
                .copied()
                .collect()
            ,
            allowed_methods: DEFAULT_ALLOWED_METHODS
                .iter()
                .map(ToString::to_string)
                .collect()
            ,
            respect_retry_after_header: DEFAULT_RESPECT_RETRY_AFTER_HEADER,
            raise_on_status: DEFAULT_RAISE_ON_STATUS,
            raise_on_redirect: DEFAULT_RAISE_ON_REDIRECT,
            total_timeout: DEFAULT_TOTAL_TIMEOUT,
        }
    }
}