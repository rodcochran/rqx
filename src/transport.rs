use std::f64::INFINITY;
use std::time::{Duration, Instant};
use reqwest::{Client, Request};
use pyo3::prelude::{PyRef, PyResult, Python,  pyclass, pymethods};
use tokio;

use super::exceptions::*;
use super::response::PyResponse;
use super::retry::PyRetry;
use super::runtime::RUNTIME;

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct HTTPTransport {
    http_client: Client,
    #[pyo3(get)]
    retries: Option<PyRetry>
}

#[pymethods]
impl HTTPTransport {
    #[new]
    #[pyo3(signature = (retries=None))]
    fn __new__(
        retries: Option<PyRef<'_, PyRetry>>,
    ) -> PyResult<Self> {

        // need to make it so that transport only creates default 
        // when someone passes a blank (not null) Retry object.
        // Default behavior is no retry.
        let _retries = match retries {
            Some(r) => Some(r.clone()),
            // None => PyRetry::with_defaults(),
            None => None
        };

        let http_client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("Failed to build HTTP client");
        Ok(Self {
            http_client: http_client,
            retries: _retries
        })
    }
}

impl Default for HTTPTransport {
    fn default() -> Self {
        Self {
            http_client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("Failed to build HTTP client"),
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

        let response = py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| ReqxError::new_err("runtime not initialized"))?
                .block_on(async {
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
        PyResponse::from_response(py, response)
    }

    fn send_with_retries(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        // Retry configuration
        let r = self.retries.as_ref().unwrap();
        let method = request.method().to_string();
        let is_retryable_method = r.allowed_methods.contains(&method);
        let backoff_max: f32 = r.backoff_max.into();
        let respect_retry = r.respect_retry_after_header;
        let total_timeout: f64 = r.total_timeout.unwrap_or(INFINITY);

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
                num_retries = num_retries + 1;

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
    #[pyo3(get)]
    retries: Option<PyRetry>
}

#[pymethods]
impl AsyncHTTPTransport {
    #[new]
    #[pyo3(signature = (retries=None))]
    fn __new__(
        retries: Option<PyRef<'_, PyRetry>>,
    ) -> PyResult<Self> {

        // need to make it so that transport only creates default 
        // when someone passes a blank (not null) Retry object.
        // Default behavior is no retry.
        let _retries = match retries {
            Some(r) => Some(r.clone()),
            // None => PyRetry::with_defaults(),
            None => None
        };

        let http_client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("Failed to build HTTP client");
        Ok(Self {
            http_client: http_client,
            retries: _retries
        })
    }
}

impl Default for AsyncHTTPTransport {
    fn default() -> Self {
        Self {
            http_client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("Failed to build Async HTTP client"),
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
            Self::send(transport.client(), request).await
        }
    }

    async fn send(client: &Client, request: Request) -> PyResult<PyResponse> {
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
        let resp_future = PyResponse::from_response_async(response);
        resp_future.await
    }
    async fn send_with_retries(transport: &AsyncHTTPTransport, request: Request) -> PyResult<PyResponse> {
        // Retry configuration
        let r = transport.retries.as_ref().unwrap();
        let method = request.method().to_string();
        let is_retryable_method = r.allowed_methods.contains(&method);
        let backoff_max: f32 = r.backoff_max.into();
        let respect_retry = r.respect_retry_after_header;
        let total_timeout: f64 = r.total_timeout.unwrap_or(INFINITY);

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
                num_retries = num_retries + 1;

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
            match Self::send(transport.client(), request_copy).await {
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
}