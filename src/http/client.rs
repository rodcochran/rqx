// Shared client core for PyClient and PyAsyncClient.
//
// All methods are pure-Rust async — no pyo3 ceremony in bodies. The pyo3
// boundary (Bound<PyAny>, py.detach, future_into_py) lives entirely in the
// pyclass wrappers in src/client.rs.
//
// Cookies use Arc<TokioMutex> so both the sync and async pyclass wrappers
// can share this exact type. TokioMutex::blocking_lock() is safe from the
// sync side (which calls from outside any tokio runtime).

use http::{HeaderMap, Method};
use pyo3::Python;
use pyo3::prelude::PyResult;
use reqwest::{Request, Response};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex as TokioMutex;
use url::Url;

use crate::exceptions::*;
use crate::http::transport::Transport;
use crate::request::{
    build_client_request, build_redirect_request, determine_redirect_method, determine_redirect_url,
};
use crate::response::PyResponse;
use crate::retry::DEFAULT_RAISE_ON_REDIRECT;

#[derive(Clone)]
pub struct Client {
    transport: Transport,
    timeout_secs: f64,
    follow_redirects: bool,
    max_redirects: u32,
    base_url: Option<Url>,
    cookies: Arc<TokioMutex<HashMap<String, String>>>,
}

impl Client {
    pub fn new(
        transport: Transport,
        timeout_secs: f64,
        follow_redirects: bool,
        max_redirects: u32,
        base_url: Option<Url>,
    ) -> Self {
        Self {
            transport,
            timeout_secs,
            follow_redirects,
            max_redirects,
            base_url,
            cookies: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    // ── Read-only accessors ────────────────────────────────────────────

    pub fn base_url(&self) -> Option<String> {
        self.base_url.as_ref().map(|u| u.to_string())
    }

    pub fn timeout_secs(&self) -> f64 {
        self.timeout_secs
    }

    /// Sync snapshot of the cookie jar — safe to call from any context.
    /// Uses blocking_lock since pyclass `#[getter]`s are called from sync
    /// Python attribute access, never from inside an async future.
    pub fn cookies_snapshot(&self) -> HashMap<String, String> {
        self.cookies.blocking_lock().clone()
    }

    // ── Core request dispatch ──────────────────────────────────────────

    pub async fn request(
        &self,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        let start_time = Instant::now();

        let resolved_url = crate::url::resolve_url(self.base_url.as_ref(), url)?;
        let request = build_client_request(
            self.transport.client(),
            method,
            &resolved_url,
            content,
            data,
            json.as_ref(),
            params,
            headers,
            auth,
            timeout,
        )?;

        let follow = follow_redirects.unwrap_or(self.follow_redirects);
        let mut resp = if follow {
            self.send_handling_redirects(request).await?
        } else {
            self.transport.handle_request(request).await?
        };

        self.accumulate_cookies(&resp.cookies).await;

        resp.elapsed = (Instant::now() - start_time).as_secs_f64();
        Ok(resp)
    }

    pub async fn stream(
        &self,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<(Response, f64)> {
        // Returns (response, elapsed_secs) — the pyclass wraps the Response
        // into PyStreamResponse / PyAsyncStreamResponse and sets elapsed.
        let start_time = Instant::now();

        let resolved_url = crate::url::resolve_url(self.base_url.as_ref(), url)?;
        let request = build_client_request(
            self.transport.client(),
            method,
            &resolved_url,
            content,
            data,
            json.as_ref(),
            params,
            headers,
            auth,
            timeout,
        )?;

        let follow = follow_redirects.unwrap_or(self.follow_redirects);
        let response = if follow {
            self.send_handling_redirects_stream(request).await?
        } else {
            self.transport.send_raw(request).await?
        };

        // Accumulate cookies from the final response. (Intermediate-hop
        // cookies were already accumulated by send_handling_redirects_stream.)
        // .cookies() iterates Set-Cookie headers without consuming the body.
        let final_cookies: HashMap<String, String> = response
            .cookies()
            .map(|c| (c.name().to_string(), c.value().to_string()))
            .collect();
        self.accumulate_cookies(&final_cookies).await;

        let elapsed = (Instant::now() - start_time).as_secs_f64();
        Ok((response, elapsed))
    }

    // ── HTTP method shortcuts ──────────────────────────────────────────

    pub async fn get(
        &self,
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        self.request(
            "GET",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn options(
        &self,
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        self.request(
            "OPTIONS",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn head(
        &self,
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        self.request(
            "HEAD",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn delete(
        &self,
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        self.request(
            "DELETE",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn post(
        &self,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        self.request(
            "POST",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn put(
        &self,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        self.request(
            "PUT",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn patch(
        &self,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        self.request(
            "PATCH",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
        .await
    }

    // ── Internal redirect + cookie helpers ─────────────────────────────

    /// Merge response cookies into the jar. Skips the lock when the response
    /// has no cookies (the common case) so the jar is only a serialization
    /// point on responses that actually set cookies.
    async fn accumulate_cookies(&self, resp_cookies: &HashMap<String, String>) {
        if resp_cookies.is_empty() {
            return;
        }
        self.cookies
            .lock()
            .await
            .extend(resp_cookies.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    async fn send_handling_redirects(&self, request: Request) -> PyResult<PyResponse> {
        let original_method = request.method().clone();
        let original_url = request.url().clone();
        let original_headers = request.headers().clone();
        let mut current_response = self.transport.handle_request(request).await?;
        // Capture cookies from the first response before following the redirect
        // chain — intermediate hops can set cookies we'd otherwise drop.
        self.accumulate_cookies(&current_response.cookies).await;

        for _ in 1..self.max_redirects {
            if !(300..400).contains(&current_response.status_code) {
                return Ok(current_response);
            }
            current_response = self
                .handle_redirect(
                    &original_url,
                    &original_method,
                    &original_headers,
                    &current_response,
                )
                .await?;
            self.accumulate_cookies(&current_response.cookies).await;
        }

        if (300..400).contains(&current_response.status_code) {
            let raise = self
                .transport
                .retries
                .as_ref()
                .map(|r| r.raise_on_redirect)
                .unwrap_or(DEFAULT_RAISE_ON_REDIRECT);
            if raise {
                return Err(TooManyRedirects::new_err(format!(
                    "Exceeded max redirects {}",
                    self.max_redirects
                )));
            }
        }
        Ok(current_response)
    }

    async fn send_handling_redirects_stream(&self, request: Request) -> PyResult<Response> {
        let original_method = request.method().clone();
        let original_url = request.url().clone();
        let original_headers = request.headers().clone();

        let raise_on_redirect = self
            .transport
            .retries
            .as_ref()
            .map(|r| r.raise_on_redirect)
            .unwrap_or(DEFAULT_RAISE_ON_REDIRECT);

        let mut current_request = request;
        let mut redirects_used: u32 = 0;
        loop {
            let response = self.transport.send_raw(current_request).await?;
            let status = response.status().as_u16();

            if !(300..400).contains(&status) {
                return Ok(response);
            }

            if redirects_used + 1 >= self.max_redirects {
                if raise_on_redirect {
                    return Err(TooManyRedirects::new_err(format!(
                        "Exceeded max redirects {}",
                        self.max_redirects
                    )));
                }
                return Ok(response);
            }

            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(String::from)
                .ok_or_else(|| RqxError::new_err("3xx response missing Location header"))?;

            let cookies_map: HashMap<String, String> = response
                .cookies()
                .map(|c| (c.name().to_string(), c.value().to_string()))
                .collect();
            self.accumulate_cookies(&cookies_map).await;

            // Drain to release the connection back to the pool.
            let _ = response.bytes().await;

            let new_url = determine_redirect_url(&original_url, &location)
                .map_err(|e| RqxError::new_err(format!("Error parsing url from redirect: {e}")))?;
            let new_method = determine_redirect_method(&original_method, status);
            current_request = build_redirect_request(
                self.transport.client(),
                new_method,
                new_url,
                &original_headers,
            );

            redirects_used += 1;
        }
    }

    async fn handle_redirect(
        &self,
        original_url: &Url,
        original_method: &Method,
        original_headers: &HeaderMap,
        resp: &PyResponse,
    ) -> PyResult<PyResponse> {
        // PyResponse.headers is a Py<PyHeaders>, which requires the GIL to read.
        // Python::attach acquires it briefly to pull the Location string out.
        let location = Python::attach(|py| {
            resp.headers
                .borrow(py)
                .get_first("location")
                .map(String::from)
        })
        .ok_or_else(|| RqxError::new_err("3xx response missing Location header"))?;
        let new_url = determine_redirect_url(original_url, &location)
            .map_err(|e| RqxError::new_err(format!("Error parsing url from redirect: {e}")))?;

        let new_method = determine_redirect_method(original_method, resp.status_code);
        let current_request = build_redirect_request(
            self.transport.client(),
            new_method,
            new_url,
            original_headers,
        );
        self.transport.handle_request(current_request).await
    }
}
