
use std::time::Duration;

use reqwest::{Client, Request};
use pyo3::prelude::{PyRef, PyResult, Python,  pyclass, pymethods};
use tokio;

use super::exceptions::*;
use super::response::PyResponse;
use super::retry::PyRetry;
use super::runtime::RUNTIME;

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct HttpTransport {
    http_client: Client,
    #[pyo3(get)]
    retries: Option<PyRetry>
}

#[pymethods]
impl HttpTransport {
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

impl Default for HttpTransport {
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

impl HttpTransport {
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
        /*
            1. Sends the request
            2. Checks if the response should be retried (status in forcelist, method in allowed_methods)
            3. If yes, sleeps (backoff) and retries
            4. Tracks attempt count and history
            5. Returns the final response or raises MaxRetriesExceeded 
        */

        // get reference to retry object
        let r = self.retries.as_ref().unwrap();
        let method = request.method().to_string();

        // try_clone can return None - should make sure its handled.
        let mut request_copy = request
            .try_clone()
            .ok_or_else(
                || ReqxError::new_err("Streaming request bodies cannot be retried")
            )?;

        let mut current_response = self.send(py, request_copy)
            .map_err(|e| {
                ReqxError::new_err(format!("request failed: {e}"))
            })?;
        // let mut retry_history = ();

        if !r.allowed_methods.contains(&method){
            return Ok(current_response);
        }

        let mut backoff_time: f32;

        for attempt in 0..r.total {
            if !r.status_forcelist.contains(&current_response.status_code) {
                return Ok(current_response);
            }

            // now retry is confirmed to start.
            // Calculate backoff time
            backoff_time = f32::min(r.backoff_factor * 2_f32.powi(attempt), r.backoff_max.into());

            // Run tokio sleep in Rust's runtime
            py.detach(|| {
                RUNTIME
                    .get()
                    .expect("runtime not initialized")
                    .block_on(async {
                        tokio::time::sleep(Duration::from_secs_f32(backoff_time)).await
                    })
            });

            request_copy = request
                .try_clone()
                .ok_or_else(
                    || ReqxError::new_err("Streaming request bodies cannot be retried")
                )?;
            current_response = self.send(py, request_copy)
                .map_err(|e| {
                    ReqxError::new_err(format!("request failed: {e}"))
                })?;
        }

        if r.status_forcelist.contains(&current_response.status_code) {
            return Err(
                MaxRetriesExceeded::new_err(format!("max retries exeeded: {}", r.total))
            )
        }
        return Ok(current_response);

    }
    
}


/* 
Accessors etc.
*/
impl HttpTransport {
    pub fn client(&self) -> &Client {
        &self.http_client
    }
}