use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use reqwest::{Client, Request, Response};
use reqwest::tls::Certificate;
use pyo3::prelude::{PyRef, PyResult, Python,  pyclass, pymethods};
use pyo3::types::{PyAny, PyAnyMethods, PyBool, PyString};
use pyo3::Bound;
use tokio::sync::{Semaphore};

use super::exceptions::*;
use super::response::PyResponse;
use super::retry::PyRetry;
use super::runtime::RUNTIME;


#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct HTTPTransport {
    http_client: Client,
    // None when the user didn't pass max_connections — skips the atomic on
    // every request. Some(sem) enforces the user-provided cap.
    max_connection_semaphore: Option<Arc<Semaphore>>,
    #[pyo3(get)]
    retries: Option<PyRetry>
}

#[pymethods]
impl HTTPTransport {
    #[new]
    #[pyo3(signature = (
        retries=None, 
        max_connections=None, 
        max_keepalive_connections=None, 
        keepalive_expiry=None, 
        http2=None,
        verify=None,
        proxy=None,
    ))]
    fn __new__(
        retries: Option<PyRef<'_, PyRetry>>,
        max_connections: Option<u32>,
        max_keepalive_connections: Option<u32>,
        keepalive_expiry: Option<f64>,
        http2: Option<bool>,
        verify: Option<&Bound<'_, PyAny>>,
        proxy: Option<HashMap<String, String>>
    ) -> PyResult<Self> {

        // need to make it so that transport only creates default 
        // when someone passes a blank (not null) Retry object.
        // Default behavior is no retry.
        let _retries = match retries {
            Some(r) => Some(r.clone()),
            // None => PyRetry::with_defaults(),
            None => None
        };

        let http_client = build_http_client(
            max_keepalive_connections, 
            keepalive_expiry, 
            http2, 
            verify, 
            proxy
        )?;

        let max_connection_semaphore = max_connections
            .map(|mc| Arc::new(Semaphore::new(mc as usize)));

        Ok(Self {
            http_client: http_client,
            max_connection_semaphore: max_connection_semaphore,
            retries: _retries
        })
    }
}

impl Default for HTTPTransport {
    fn default() -> Self {
        Self {
            http_client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .cookie_store(true)
                .build()
                .expect("Failed to build HTTP client"),
            max_connection_semaphore: None,
            retries: None
        }
    }
}

impl HTTPTransport {
    pub fn handle_request(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        if self.retries.is_some() {
            // Handle retries etc.
            self.send_with_retries(py, request)
        } else {
            // don't do any retries
            self.send(py, request)
        }   
    }

    fn send(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        let response= self.send_raw(py, request)?;
        PyResponse::from_response(py, response)
    }

    pub fn send_raw(&self, py: Python<'_>, request: Request) -> PyResult<Response> {
        let response = py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| ReqxError::new_err("runtime not initialized"))?
                // NOTE: block_on panics if called from within an existing tokio runtime
                // context ("Cannot start a runtime from within a runtime"). Safe here
                // because Python is the caller and py.detach releases the GIL without
                // entering a runtime. Callers embedding this in an async Python framework
                // (or invoking it from inside another tokio task) will panic — they should
                // use the async variant of this API instead.
                .block_on(async {
                    // Acquire a permit only when max_connections is set; otherwise skip the
                    // atomic entirely. _permit lives to end of block regardless.
                    let _permit = match self.max_connection_semaphore.as_ref() {
                        Some(sem) => Some(
                            sem.acquire()
                                .await
                                .map_err(|_| ReqxError::new_err("connection pool closed"))?,
                        ),
                        None => None,
                    };
                    self.http_client
                        .execute(request)
                        .await
                        .map_err(|e| {
                            if e.is_timeout() {
                                TimeoutException::new_err(format!("request timed out: {e}"))
                            } else {
                                ReqxError::new_err(format!("request failed: {e}"))
                            }
                        })
                })
        })?;
        return Ok(response);
    }

    fn send_with_retries(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        // Retry configuration
        let r = self.retries.as_ref().unwrap();
        let method = request.method().to_string();
        let is_retryable_method = r.allowed_methods.contains(&method);
        let backoff_max: f32 = r.backoff_max.into();
        let respect_retry = r.respect_retry_after_header;
        let total_timeout: f64 = r.total_timeout.unwrap_or(f64::INFINITY);

        // Stateful components to track
        let mut num_retries: i32 = 0;
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        let mut current_response: Option<PyResponse> = None;
        let mut request_copy: Request;
        
        // Timer for total time in retry
        let start_time = Instant::now();

        for attempt in 0..=r.total {
            if start_time.elapsed().as_secs_f64() > total_timeout {
                return Err(
                    MaxRetriesExceeded::new_err(
                        format!(
                            "total timeout of {}s exceeded after {} retries", 
                            total_timeout,
                            num_retries,
                        )
                    )
                )
            }
            if attempt > 0 {
                // increment retries
                num_retries += 1;

                let retry_after: f32 = if respect_retry {
                    current_response.as_ref()
                        .and_then(|r| r.headers.get("retry-after"))
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
              
                let calculated_backoff = r.backoff_factor * 2_f32.powi(attempt);
                let backoff_time = f32::min(f32::max(calculated_backoff, retry_after), backoff_max);
                // Run tokio sleep in Rust's runtime
                py.detach(|| {
                    RUNTIME
                        .get()
                        .expect("runtime not initialized")
                        .block_on(async {
                            tokio::time::sleep(Duration::from_secs_f32(backoff_time)).await
                        })
                });
            }            
            request_copy = request
                .try_clone()
                .ok_or_else(
                    || ReqxError::new_err("Streaming request bodies cannot be retried")
                )?;
            
            let attempt_start = std::time::Instant::now();
            match self.send(py, request_copy) {
                Ok(resp) => {
                    if !is_retryable_method { 
                        return Ok(resp);
                    }

                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    if attempt > 0 {
                        retry_history.push((resp.status_code.to_string(), attempt_elapsed));
                    }
                    current_response = Some(resp);

                },
                Err(e) => {
                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    current_response = None;
                    // record in retry_history
                    if attempt > 0 {
                        retry_history.push((format!("{}", e), attempt_elapsed));
                    }
                }
            }

            if let Some(cr) = current_response.as_ref() {
                if !r.status_forcelist.contains(&cr.status_code) {
                    // need to take ownership here to set fields and return
                    let mut resp = current_response.unwrap();
                    resp.num_retries = num_retries;
                    resp.retry_history = retry_history;
                    return Ok(resp);
                }
            }
        }

        match current_response {
            Some(mut cr) => {
                if r.status_forcelist.contains(&cr.status_code) {
                    return Err(
                        MaxRetriesExceeded::new_err(format!("max retries exeeded: {}", r.total))
                    )
                }
                cr.num_retries = num_retries;
                cr.retry_history = retry_history;
                return Ok(cr);
            }
            None => {
                return Err(
                    MaxRetriesExceeded::new_err(format!("max retries exeeded: {}", r.total))
                )
            }
        }
    }

}


/* 
Accessors etc.
*/
impl HTTPTransport {
    pub fn client(&self) -> &Client {
        &self.http_client
    }
}

/*
Async-compatible HTTP Transport
*/

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct AsyncHTTPTransport {
    http_client: Client,
    // None when the user didn't pass max_connections — skips the atomic on
    // every request. Some(sem) enforces the user-provided cap.
    max_connection_semaphore: Option<Arc<Semaphore>>,
    #[pyo3(get)]
    retries: Option<PyRetry>
}

#[pymethods]
impl AsyncHTTPTransport {
    #[new]
    #[pyo3(signature = (
        retries=None, 
        max_connections=None, 
        max_keepalive_connections=None, 
        keepalive_expiry=None,
        http2=None,
        verify=None,
        proxy=None,
    ))]
    fn __new__(
        retries: Option<PyRef<'_, PyRetry>>,
        max_connections: Option<u32>,
        max_keepalive_connections: Option<u32>,
        keepalive_expiry: Option<f64>,
        http2: Option<bool>,
        verify: Option<&Bound<'_, PyAny>>,
        proxy: Option<HashMap<String, String>>
    ) -> PyResult<Self> {

        // need to make it so that transport only creates default 
        // when someone passes a blank (not null) Retry object.
        // Default behavior is no retry.
        let _retries = match retries {
            Some(r) => Some(r.clone()),
            // None => PyRetry::with_defaults(),
            None => None
        };

        let http_client = build_http_client(
            max_keepalive_connections, 
            keepalive_expiry, 
            http2, 
            verify, 
            proxy
        )?;

        let max_connection_semaphore = max_connections
            .map(|mc| Arc::new(Semaphore::new(mc as usize)));

        Ok(Self {
            http_client: http_client,
            max_connection_semaphore: max_connection_semaphore,
            retries: _retries
        })
    }
}

impl Default for AsyncHTTPTransport {
    fn default() -> Self {
        Self {
            http_client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .cookie_store(true)
                .build()
                .expect("Failed to build Async HTTP client"),
            max_connection_semaphore: None,
            retries: None
        }
    }
}

impl AsyncHTTPTransport {
    pub async fn handle_request(transport: AsyncHTTPTransport, request: Request) -> PyResult<PyResponse> {
        if transport.retries.is_some() {
            // Handle retries etc.
            Self::send_with_retries(&transport, request).await
        } else {
            // don't do any retries
            Self::send(transport.client(), request, transport.semaphore()).await
        }
    }

    async fn send(
        client: &Client,
        request: Request,
        max_connection_semaphore: &Option<Arc<Semaphore>>,
    ) -> PyResult<PyResponse> {
        let response = Self::send_raw(client, request, max_connection_semaphore).await?;
        PyResponse::from_response_async(response).await
    }

    pub async fn send_raw(
        client: &Client,
        request: Request,
        max_connection_semaphore: &Option<Arc<Semaphore>>,
    ) -> PyResult<Response> {
        // Acquire a permit only when max_connections is set; otherwise skip the
        // atomic entirely. _permit lives to end of scope regardless.
        let _permit = match max_connection_semaphore.as_ref() {
            Some(sem) => Some(
                sem.acquire()
                    .await
                    .map_err(|_| ReqxError::new_err("connection pool closed"))?,
            ),
            None => None,
        };
        let response = client
            .execute(request)
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    TimeoutException::new_err(format!("request timed out: {e}"))
                } else {
                    ReqxError::new_err(format!("request failed: {e}"))
                }
            }
        )?;
        return Ok(response);
    }

    async fn send_with_retries(transport: &AsyncHTTPTransport, request: Request) -> PyResult<PyResponse> {
        // Retry configuration
        let r = transport.retries.as_ref().unwrap();
        let method = request.method().to_string();
        let is_retryable_method = r.allowed_methods.contains(&method);
        let backoff_max: f32 = r.backoff_max.into();
        let respect_retry = r.respect_retry_after_header;
        let total_timeout: f64 = r.total_timeout.unwrap_or(f64::INFINITY);

        // Stateful components to track
        let mut num_retries: i32 = 0;
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        let mut current_response: Option<PyResponse> = None;
        let mut request_copy: Request;

        // Timer for total time in retry
        let start_time = Instant::now();

        for attempt in 0..=r.total {
            if start_time.elapsed().as_secs_f64() > total_timeout {
                return Err(
                    MaxRetriesExceeded::new_err(
                        format!(
                            "total timeout of {}s exceeded after {} retries", 
                            total_timeout,
                            num_retries,
                        )
                    )
                )
            }

            if attempt > 0 {
                // increment retries
                num_retries += 1;

                let retry_after: f32 = if respect_retry {
                    current_response.as_ref()
                        .and_then(|r| r.headers.get("retry-after"))
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
              
                let calculated_backoff = r.backoff_factor * 2_f32.powi(attempt);
                let backoff_time = f32::min(f32::max(calculated_backoff, retry_after), backoff_max);

                // Run tokio sleep in Rust's runtime
                tokio::time::sleep(Duration::from_secs_f32(backoff_time)).await
            }

            request_copy = request
                .try_clone()
                .ok_or_else(
                    || ReqxError::new_err("Streaming request bodies cannot be retried")
                )?;
            
            let attempt_start = std::time::Instant::now();
            match Self::send(transport.client(), request_copy, transport.semaphore()).await {
                Ok(resp) => {
                    if !is_retryable_method { 
                        return Ok(resp);
                    }

                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    if attempt > 0 {
                        retry_history.push((resp.status_code.to_string(), attempt_elapsed));
                    }
                    current_response = Some(resp);

                },
                Err(e) => {
                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    current_response = None;
                    // record in retry_history
                    if attempt > 0 {
                        retry_history.push((format!("{}", e), attempt_elapsed));
                    }
                }
            }

            if let Some(cr) = current_response.as_ref() {
                if !r.status_forcelist.contains(&cr.status_code) {
                    // need to take ownership here to set fields and return
                    let mut resp = current_response.unwrap();
                    resp.num_retries = num_retries;
                    resp.retry_history = retry_history;
                    return Ok(resp);
                }
            }
            
            
        }

        match current_response {
            Some(mut cr) => {
                if r.status_forcelist.contains(&cr.status_code) {
                    return Err(
                        MaxRetriesExceeded::new_err(format!("max retries exeeded: {}", r.total))
                    )
                }
                cr.num_retries = num_retries;
                cr.retry_history = retry_history;
                return Ok(cr);
            }
            None => {
                return Err(
                    MaxRetriesExceeded::new_err(format!("max retries exeeded: {}", r.total))
                )
            }
        }
    }
}

/* 
Accessors etc.
*/
impl AsyncHTTPTransport {
    pub fn client(&self) -> &Client {
        &self.http_client
    }

    pub fn semaphore(&self) -> &Option<Arc<Semaphore>> {
        &self.max_connection_semaphore
    }
}

/*

Helper for constructing the HTTP Client

*/
fn build_http_client(
    max_keepalive_connections: Option<u32>,
    keepalive_expiry: Option<f64>,
    http2: Option<bool>,
    verify: Option<&Bound<'_, PyAny>>,
    proxy: Option<HashMap<String, String>>
) -> PyResult<Client> {
    let mut http_client_builder =  Client::builder()
        // Explicitly add no redirects at the transport level, as we let the PyClient take care of it
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true);

    if let Some(max_keepalive) = max_keepalive_connections {
        http_client_builder = http_client_builder.pool_max_idle_per_host(max_keepalive as usize);
    }

    if let Some(ke) = keepalive_expiry {
        http_client_builder = http_client_builder.pool_idle_timeout(Duration::from_secs_f64(ke));
    }

    if http2.unwrap_or(false) {
        http_client_builder = http_client_builder.http2_prior_knowledge();
    } else {
        http_client_builder = http_client_builder.http1_only();
    }

    if let Some(v) = verify {
        if v.is_instance_of::<PyBool>() {
            let verify_enabled = v.extract::<bool>().unwrap();
            if !verify_enabled {
                http_client_builder = http_client_builder.danger_accept_invalid_certs(true);
            }
        }
        else if v.is_instance_of::<PyString>() {
            let path = v
                .extract::<String>()
                .map_err(|e| ReqxError::new_err(format!("failed to parse CA cert path: {e}")))?;
            let bytes = std::fs::read(&path)
                .map_err(|e| ReqxError::new_err(format!("failed to read CA cert: {e}")))?;
            let cert = Certificate::from_pem(&bytes)
                .map_err(|e| ReqxError::new_err(format!("failed to construct CA cert: {e}")))?;

            http_client_builder = http_client_builder.add_root_certificate(cert);
        }
    }

    if let Some(proxies) = proxy {
        for (scheme, url) in proxies {
            let p = match scheme.as_str() {
                "http" => reqwest::Proxy::http(&url),
                "https" => reqwest::Proxy::https(&url),
                _ => continue,
            }.map_err(|e| ReqxError::new_err(format!("invalid proxy: {e}")))?;
            http_client_builder = http_client_builder.proxy(p);
        }
    }

    let http_client = http_client_builder
        .build()
        .expect("Failed to build HTTP client");

    return Ok(http_client);

}