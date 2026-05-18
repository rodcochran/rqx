use pyo3::Bound;
use pyo3::prelude::{PyRef, PyResult, Python, pyclass, pymethods};
use pyo3::types::PyAny;
use reqwest::tls::Identity;
use reqwest::{Client, ClientBuilder, Request, Response};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::exceptions::*;
use crate::http::protocol::HttpVersionConfig;
use crate::http::proxy::parse_proxies;
use crate::http::tls::{VerifyConfig, parse_identity};
use crate::response::PyResponse;
use crate::retry::PyRetry;
use crate::runtime::RUNTIME;
use crate::timeout::PyTimeout;

// ────────────────────────────────────────────────────────────────────────
// Transport — shared pure-Rust core for HTTPTransport and AsyncHTTPTransport.
//
// Handles request execution, connection-pool gating, and the retry state
// machine. Pure async — no pyo3 ceremony in bodies. The pyo3 boundary
// (block_on / .await) lives in the pyclass wrappers below.
// ────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Transport {
    client: Client,
    semaphore: Option<Arc<Semaphore>>,
    pub retries: Option<PyRetry>,
}

impl Transport {
    pub fn new(
        client: Client,
        semaphore: Option<Arc<Semaphore>>,
        retries: Option<PyRetry>,
    ) -> Self {
        Self {
            client,
            semaphore,
            retries,
        }
    }

    /// Picks retry vs. no-retry path.
    pub async fn handle_request(&self, request: Request) -> PyResult<PyResponse> {
        if self.retries.is_some() {
            self.send_with_retries(request).await
        } else {
            self.send(request).await
        }
    }

    /// Single-attempt — returns the deserialized Python response.
    async fn send(&self, request: Request) -> PyResult<PyResponse> {
        let response = self.send_raw(request).await?;
        PyResponse::from_response_async(response).await
    }

    /// Single-attempt — returns the raw reqwest Response (escape hatch / streaming).
    pub async fn send_raw(&self, request: Request) -> PyResult<Response> {
        let _permit = match self.semaphore.as_ref() {
            Some(sem) => Some(
                sem.acquire()
                    .await
                    .map_err(|_| RqxError::new_err("connection pool closed"))?,
            ),
            None => None,
        };
        let response = self
            .client
            .execute(request)
            .await
            .map_err(map_reqwest_error)?;
        Ok(response)
    }

    /// The retry state machine.
    async fn send_with_retries(&self, request: Request) -> PyResult<PyResponse> {
        // Operates on raw reqwest::Response throughout — reading status and
        // retry-after directly from response headers without acquiring the GIL.
        // PyResponse construction happens only at the final return points.
        // Mirrors the redirect-loop fix from #93.
        let r = self.retries.as_ref().unwrap();
        let method = request.method().to_string();
        let is_retryable_method = r.allowed_methods.contains(&method);
        let backoff_max: f32 = r.backoff_max;
        let respect_retry = r.respect_retry_after_header;
        let total_timeout: f64 = r.total_timeout.unwrap_or(f64::INFINITY);

        let mut num_retries: i32 = 0;
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        let mut current_response: Option<Response> = None;
        let mut request_copy: Request;

        let start_time = Instant::now();

        for attempt in 0..=r.total {
            if start_time.elapsed().as_secs_f64() > total_timeout {
                return Err(MaxRetriesExceeded::new_err(format!(
                    "total timeout of {}s exceeded after {} retries",
                    total_timeout, num_retries,
                )));
            }

            if attempt > 0 {
                num_retries += 1;

                let retry_after: f32 = if respect_retry {
                    current_response
                        .as_ref()
                        .and_then(|resp| {
                            resp.headers()
                                .get("retry-after")
                                .and_then(|v| v.to_str().ok())
                                .map(String::from)
                        })
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or(0.0)
                } else {
                    0.0
                };

                let mut calculated_backoff = r.backoff_factor * 2_f32.powi(attempt - 1);
                // Apply jitter to spread out retries from many concurrent clients.
                // Multiplier: (1 + uniform(-jitter, +jitter)) — so jitter=0.5 means
                // backoff varies ±50% from the deterministic value.
                if r.backoff_jitter > 0.0 {
                    let jitter = rand::random::<f32>() * 2.0 - 1.0; // [-1, 1)
                    calculated_backoff =
                        (calculated_backoff * (1.0 + jitter * r.backoff_jitter)).max(0.0);
                }
                let backoff_time = f32::min(f32::max(calculated_backoff, retry_after), backoff_max);

                tokio::time::sleep(Duration::from_secs_f32(backoff_time)).await
            }

            // Drain the previous attempt's body to release the connection back
            // to the pool before issuing the next request. Required because we
            // hold raw Response across iterations; PyResponse wrapping would
            // have drained implicitly via response.bytes().await.
            if let Some(old) = current_response.take() {
                let _ = old.bytes().await;
            }

            request_copy = request
                .try_clone()
                .ok_or_else(|| RqxError::new_err("Streaming request bodies cannot be retried"))?;

            let attempt_start = Instant::now();
            match self.send_raw(request_copy).await {
                Ok(resp) => {
                    if !is_retryable_method {
                        return PyResponse::from_response_async(resp).await;
                    }

                    let status = resp.status().as_u16();
                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    if attempt > 0 {
                        retry_history.push((status.to_string(), attempt_elapsed));
                    }

                    if !r.status_forcelist.contains(&status) {
                        let mut response = PyResponse::from_response_async(resp).await?;
                        response.num_retries = num_retries;
                        response.retry_history = retry_history;
                        return Ok(response);
                    }

                    current_response = Some(resp);
                }
                Err(e) => {
                    if !is_retryable_method {
                        return Err(e);
                    }
                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    if attempt > 0 {
                        retry_history.push((format!("{}", e), attempt_elapsed));
                    }
                    current_response = None;
                }
            }
        }

        match current_response {
            Some(cr) => {
                // When status_forcelist matched and retries were exhausted:
                // raise_on_status=true (default) → raise MaxRetriesExceeded
                // raise_on_status=false → return the failing response so the
                //   caller can inspect status_code / headers / body.
                let status = cr.status().as_u16();
                if r.status_forcelist.contains(&status) && r.raise_on_status {
                    return Err(MaxRetriesExceeded::new_err(format!(
                        "max retries exceeded: {}",
                        r.total
                    )));
                }
                let mut response = PyResponse::from_response_async(cr).await?;
                response.num_retries = num_retries;
                response.retry_history = retry_history;
                Ok(response)
            }
            None => Err(MaxRetriesExceeded::new_err(format!(
                "max retries exceeded: {}",
                r.total
            ))),
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl Default for Transport {
    fn default() -> Self {
        let client = build_http_client(None, None, None, None, None, None, None, None)
            .expect("Error building http client");

        Self {
            client,
            semaphore: None,
            retries: None,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// RqxClientBuilder — wraps reqwest's ClientBuilder with rqx's config vocab.
//
// Methods are sliced by *what they configure on reqwest*, not by *which
// Python argument they came from* — each concern owns exactly one set of
// reqwest setters and there are no inter-method collisions.
//
// All `with_*` methods consume and return `Self` to support chaining and
// are infallible: parsing/validation happens upstream in `build_http_client`.
// ────────────────────────────────────────────────────────────────────────

pub struct RqxClientBuilder {
    inner: ClientBuilder,
}

impl RqxClientBuilder {
    /// New builder seeded with rqx's baseline:
    /// - `redirect::Policy::none()` (Client layer handles redirects)
    /// - `cookie_store(true)`
    pub fn new() -> Self {
        Self {
            inner: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .cookie_store(true),
        }
    }

    /// Configures the connection pool. Owns every `pool_*` setter on reqwest.
    ///
    /// Resolves the precedence between `keepalive_expiry` and `timeout.pool`
    /// (caller passes the latter as `pool_timeout` — `keepalive_expiry` wins
    /// when both are set).
    pub fn with_pool(
        mut self,
        max_keepalive: Option<u32>,
        keepalive_expiry: Option<f64>,
        pool_timeout: Option<f64>,
    ) -> Self {
        if let Some(max_keepalive) = max_keepalive {
            self.inner = self.inner.pool_max_idle_per_host(max_keepalive as usize);
        }
        if let Some(p) = keepalive_expiry.or(pool_timeout) {
            self.inner = self.inner.pool_idle_timeout(Duration::from_secs_f64(p));
        }
        self
    }

    pub fn with_phase_timeouts(mut self, connect: Option<f64>, read: Option<f64>) -> Self {
        if let Some(c) = connect {
            self.inner = self.inner.connect_timeout(Duration::from_secs_f64(c));
        }
        if let Some(r) = read {
            self.inner = self.inner.read_timeout(Duration::from_secs_f64(r));
        }
        self
    }

    /// HTTP version selection. Takes a pre-validated [`HttpVersionConfig`];
    /// the `(false, false)` error case is caught upstream in `from_args`.
    pub fn with_http_version(mut self, cfg: HttpVersionConfig) -> Self {
        match cfg {
            HttpVersionConfig::Negotiate => {
                // No-op — reqwest's default does ALPN negotiation over TLS.
            }
            HttpVersionConfig::Http1Only => {
                self.inner = self.inner.http1_only();
            }
            HttpVersionConfig::Http2Only => {
                self.inner = self.inner.http2_prior_knowledge();
            }
        }
        self
    }

    /// TLS: CA verification and client identity.
    ///
    /// `verify` is a pre-parsed [`VerifyConfig`] sum type covering the three
    /// meaningful states of the Python `verify=` arg (default / disable /
    /// custom CA). `cert` is a pre-parsed reqwest `Identity` for mTLS.
    pub fn with_tls(mut self, verify: Option<VerifyConfig>, cert: Option<Identity>) -> Self {
        if let Some(v) = verify {
            match v {
                VerifyConfig::Default => {}
                VerifyConfig::DisableVerification => {
                    self.inner = self.inner.danger_accept_invalid_certs(true);
                }
                VerifyConfig::CustomCa(ca) => {
                    self.inner = self.inner.add_root_certificate(ca);
                }
            }
        }
        if let Some(c) = cert {
            self.inner = self.inner.identity(c);
        }
        self
    }

    /// Proxy configuration. Takes pre-parsed `reqwest::Proxy` values; URL
    /// parsing and scheme filtering happen upstream in `parse_proxies`.
    pub fn with_proxy(mut self, proxies: Vec<reqwest::Proxy>) -> Self {
        for p in proxies {
            self.inner = self.inner.proxy(p);
        }
        self
    }

    /// Finalize into a reqwest `Client`. Panics if reqwest's build fails —
    /// failure here indicates a logic error in the builder chain, not user
    /// input.
    pub fn build(self) -> Client {
        self.inner.build().expect("Failed to build HTTP client")
    }
}

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
    let proxies = parse_proxies(proxy)?;

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

    pub fn handle_request(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| RqxError::new_err("runtime not initialized"))?
                // NOTE: block_on panics if called from within an existing tokio runtime
                // context. Safe here because Python is the caller and py.detach releases
                // the GIL without entering a runtime. Callers embedding this in an async
                // Python framework (or invoking from inside another tokio task) will
                // panic — they should use the async variant instead.
                .block_on(self.inner.handle_request(request))
        })
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

    pub async fn handle_request(&self, request: Request) -> PyResult<PyResponse> {
        self.inner.handle_request(request).await
    }

    pub async fn send_raw(&self, request: Request) -> PyResult<Response> {
        self.inner.send_raw(request).await
    }

    pub fn client(&self) -> &Client {
        self.inner.client()
    }
}
