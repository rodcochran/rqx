// Transport Core - handles sending using async directly.
// Handles the retry loop

use pyo3::prelude::{PyResult, Python};
use reqwest::{Client, Request, Response};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use super::client_builder::build_http_client;
use crate::exceptions::{MaxRetriesExceeded, RqxError, map_reqwest_error};
use crate::response::PyResponse;
use crate::retry::PyRetry;

#[derive(Clone)]
pub struct Transport {
    client: Client,
    semaphore: Option<Arc<Semaphore>>,
    pub retries: Option<PyRetry>,
}

impl Transport {
    // construction
    pub fn new(
        client: Client,
        semaphore: Option<Arc<Semaphore>>,
        retries: Option<PyRetry>,
    ) -> Self {
        Self {
            client: client,
            semaphore: semaphore,
            retries: retries,
        }
    }

    // dispatch — picks retry vs. no-retry path
    pub async fn handle_request(&self, request: Request) -> PyResult<PyResponse> {
        if self.retries.is_some() {
            // Handle retries etc.
            self.send_with_retries(request).await
        } else {
            // don't do any retries
            self.send(request).await
        }
    }

    // single-attempt — returns the deserialized Python response
    async fn send(&self, request: Request) -> PyResult<PyResponse> {
        let response = self.send_raw(request).await?;
        PyResponse::from_response_async(response).await
    }

    // single-attempt — returns the raw reqwest Response (escape hatch / streaming)
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
        return Ok(response);
    }

    // the retry state machine (the ~130-line one you duplicate today)
    async fn send_with_retries(&self, request: Request) -> PyResult<PyResponse> {
        // Retry configuration
        let r = self.retries.as_ref().unwrap();
        let method = request.method().to_string();
        let is_retryable_method = r.allowed_methods.contains(&method);
        let backoff_max: f32 = r.backoff_max;
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
                return Err(MaxRetriesExceeded::new_err(format!(
                    "total timeout of {}s exceeded after {} retries",
                    total_timeout, num_retries,
                )));
            }

            if attempt > 0 {
                // increment retries
                num_retries += 1;

                let retry_after: f32 = if respect_retry {
                    current_response
                        .as_ref()
                        .and_then(|r| {
                            Python::attach(|py| {
                                r.headers
                                    .borrow(py)
                                    .get_first("retry-after")
                                    .map(String::from)
                            })
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

                // Run tokio sleep in Rust's runtime
                tokio::time::sleep(Duration::from_secs_f32(backoff_time)).await
            }

            request_copy = request
                .try_clone()
                .ok_or_else(|| RqxError::new_err("Streaming request bodies cannot be retried"))?;

            let attempt_start = std::time::Instant::now();
            match self.send(request_copy).await {
                Ok(resp) => {
                    if !is_retryable_method {
                        return Ok(resp);
                    }

                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    if attempt > 0 {
                        retry_history.push((resp.status_code.to_string(), attempt_elapsed));
                    }
                    current_response = Some(resp);
                }
                Err(e) => {
                    if !is_retryable_method {
                        return Err(e);
                    }
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
                // When status_forcelist matched and retries were exhausted:
                // raise_on_status=true (default) → raise MaxRetriesExceeded
                // raise_on_status=false → return the failing response so the
                //   caller can inspect status_code / headers / body.
                if r.status_forcelist.contains(&cr.status_code) && r.raise_on_status {
                    return Err(MaxRetriesExceeded::new_err(format!(
                        "max retries exceeded: {}",
                        r.total
                    )));
                }
                cr.num_retries = num_retries;
                cr.retry_history = retry_history;
                return Ok(cr);
            }
            None => {
                return Err(MaxRetriesExceeded::new_err(format!(
                    "max retries exceeded: {}",
                    r.total
                )));
            }
        }
    }

    // accessor — still useful for callers that want the underlying client
    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl Default for Transport {
    fn default() -> Self {
        let client = build_http_client(None, None, None, None, None, None, None, None)
            .expect("Error building http client");

        Self {
            client: client,
            semaphore: None,
            retries: None,
        }
    }
}
