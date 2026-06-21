use pyo3::Bound;
use pyo3::prelude::{Py, PyAny, PyRef, PyResult, Python, pyclass, pymethods};
use reqwest::{Request, Response};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex as TokioMutex;
use url::Url;

use crate::exceptions::*;
use crate::py_json::py_to_value;
use crate::query_params::QueryParams;
use crate::request::{
    build_client_request, build_redirect_request, determine_redirect_method, determine_redirect_url,
};
use crate::response::PyResponse;
use crate::retry::DEFAULT_RAISE_ON_REDIRECT;
use crate::runtime::RUNTIME;
use crate::stream::{PyAsyncStreamResponse, PyStreamResponse};
use crate::timeout::PyTimeout;
use crate::transport::{AsyncHTTPTransport, HTTPTransport, Transport};
use crate::url::{parse_base_url, resolve_url};

const DEFAULT_TIMEOUT: f64 = 15.0;
const DEFAULT_FOLLOW_REDIRECTS: bool = false;
const DEFAULT_MAX_REDIRECTS: u32 = 20;

// ────────────────────────────────────────────────────────────────────────
// Client — shared pure-Rust core for PyClient and PyAsyncClient.
//
// All methods are async — no pyo3 ceremony in bodies. The pyo3 boundary
// (Bound<PyAny>, py.detach, future_into_py) lives in the pyclass wrappers
// below.
//
// Cookies use Arc<TokioMutex> so both pyclass wrappers share this type.
// TokioMutex::blocking_lock() is safe from the sync side, which calls from
// outside any tokio runtime.
// ────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Client {
    transport: Transport,
    timeout_secs: f64,
    follow_redirects: bool,
    max_redirects: u32,
    base_url: Option<Url>,
    cookies: Arc<TokioMutex<HashMap<String, String>>>,
    /// Client-level default bearer token. Per-request `auth_bearer=`
    /// overrides this when provided.
    auth_bearer: Option<String>,
}

impl Client {
    pub fn new(
        transport: Transport,
        timeout_secs: f64,
        follow_redirects: bool,
        max_redirects: u32,
        base_url: Option<Url>,
        auth_bearer: Option<String>,
    ) -> Self {
        Self {
            transport,
            timeout_secs,
            follow_redirects,
            max_redirects,
            base_url,
            cookies: Arc::new(TokioMutex::new(HashMap::new())),
            auth_bearer,
        }
    }

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

    pub async fn request(
        &self,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<PyResponse> {
        let start_time = Instant::now();

        // Resolve bearer: per-request override wins; otherwise fall back to
        // the client-level default. Then enforce the basic-vs-bearer collision
        // rule against the effective values that would actually be applied.
        let bearer = auth_bearer.or_else(|| self.auth_bearer.clone());
        if auth.is_some() && bearer.is_some() {
            return Err(RqxError::new_err(
                "Cannot specify both auth= (basic) and auth_bearer= on the same request",
            ));
        }

        let resolved_url = resolve_url(self.base_url.as_ref(), url)?;
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
            bearer.as_deref(),
            timeout,
        )?;

        let follow = follow_redirects.unwrap_or(self.follow_redirects);
        let mut resp = if follow {
            let raw = self.follow_redirects(request).await?;
            PyResponse::from_response(raw).await?
        } else {
            self.transport.handle_request(request).await?
        };

        self.accumulate_cookies(&resp.parts.cookies).await;

        resp.parts.elapsed = (Instant::now() - start_time).as_secs_f64();
        Ok(resp)
    }

    pub async fn stream(
        &self,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> PyResult<(Response, f64)> {
        // Returns (response, elapsed_secs) — the pyclass wraps the Response
        // into PyStreamResponse / PyAsyncStreamResponse and sets elapsed.
        let start_time = Instant::now();

        let bearer = auth_bearer.or_else(|| self.auth_bearer.clone());
        if auth.is_some() && bearer.is_some() {
            return Err(RqxError::new_err(
                "Cannot specify both auth= (basic) and auth_bearer= on the same request",
            ));
        }

        let resolved_url = resolve_url(self.base_url.as_ref(), url)?;
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
            bearer.as_deref(),
            timeout,
        )?;

        let follow = follow_redirects.unwrap_or(self.follow_redirects);
        let response = if follow {
            self.follow_redirects(request).await?
        } else {
            self.transport.send_raw(request).await?
        };

        // Accumulate cookies from the final response. (Intermediate-hop
        // cookies were already accumulated by follow_redirects.)
        // .cookies() iterates Set-Cookie headers without consuming the body.
        let final_cookies: HashMap<String, String> = response
            .cookies()
            .map(|c| (c.name().to_string(), c.value().to_string()))
            .collect();
        self.accumulate_cookies(&final_cookies).await;

        let elapsed = (Instant::now() - start_time).as_secs_f64();
        Ok((response, elapsed))
    }

    pub async fn get(
        &self,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn options(
        &self,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn head(
        &self,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
        .await
    }

    pub async fn delete(
        &self,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
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
            auth_bearer,
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
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
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
            auth_bearer,
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
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
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
            auth_bearer,
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
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
        .await
    }

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

    /// Follow the HTTP redirect chain. Returns the final `reqwest::Response`
    /// — intermediate-hop `Set-Cookie` headers are accumulated into
    /// `self.cookies` as a side effect. The caller decides whether to wrap
    /// into `PyResponse` (for `request`) or hand back the raw `Response`
    /// (for `stream`).
    ///
    /// Operates on `reqwest::Response` end-to-end so reading the Location
    /// header and Set-Cookie values requires no GIL acquisition (see #93).
    async fn follow_redirects(&self, request: Request) -> PyResult<Response> {
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

            // Accumulate cookies from this 3xx hop. .cookies() iterates
            // Set-Cookie headers without consuming the body.
            let cookies_map: HashMap<String, String> = response
                .cookies()
                .map(|c| (c.name().to_string(), c.value().to_string()))
                .collect();
            self.accumulate_cookies(&cookies_map).await;

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

            // Drain the 3xx body to release the connection back to the pool.
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
}

// ────────────────────────────────────────────────────────────────────────
// PyClient — synchronous Python-facing client
// ────────────────────────────────────────────────────────────────────────

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct PyClient {
    inner: Client,
}

#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, base_url=None, auth_bearer=None, transport=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<&Bound<'_, PyAny>>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        base_url: Option<&str>,
        auth_bearer: Option<String>,
        transport: Option<PyRef<'_, HTTPTransport>>,
    ) -> PyResult<Self> {
        let timeout_secs = PyTimeout::resolve_request_timeout(timeout, DEFAULT_TIMEOUT)?;
        let follow = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let max_r = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        let parsed_base_url = base_url.map(parse_base_url).transpose()?;

        if transport.is_some() && (verify.is_some() || cert.is_some() || timeout.is_some()) {
            return Err(RqxError::new_err(
                "Cannot specify both transport= and cert=/verify=/timeout=; pass options through one or the other".to_string(),
            ));
        }

        let transport_inner = match transport {
            Some(t) => t.inner.clone(),
            None => HTTPTransport::new(verify, cert, timeout)?.inner,
        };

        Ok(Self {
            inner: Client::new(
                transport_inner,
                timeout_secs,
                follow,
                max_r,
                parsed_base_url,
                auth_bearer,
            ),
        })
    }

    #[getter]
    fn base_url(&self) -> Option<String> {
        self.inner.base_url()
    }

    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.inner.cookies_snapshot()
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn request(
        &self,
        py: Python<'_>,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(py_to_value);
        let timeout_f64 = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner.request(
                method,
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                timeout_f64,
            ),
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn get(
        &self,
        py: Python<'_>,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner
                .get(url, params, headers, auth, auth_bearer, follow_redirects, t),
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn options(
        &self,
        py: Python<'_>,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner
                .options(url, params, headers, auth, auth_bearer, follow_redirects, t),
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn head(
        &self,
        py: Python<'_>,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner
                .head(url, params, headers, auth, auth_bearer, follow_redirects, t),
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn delete(
        &self,
        py: Python<'_>,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner
                .delete(url, params, headers, auth, auth_bearer, follow_redirects, t),
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn post(
        &self,
        py: Python<'_>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner.post(
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn put(
        &self,
        py: Python<'_>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner.put(
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn patch(
        &self,
        py: Python<'_>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner.patch(
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn stream(
        &self,
        py: Python<'_>,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyStreamResponse> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let (response, elapsed) = block_on_inner(
            py,
            self.inner.stream(
                method,
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                t,
            ),
        )?;
        let mut resp = PyStreamResponse::from_response(response)?;
        resp.parts.elapsed = elapsed;
        Ok(resp)
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) {
        // No-op exit since reqwest client manages an Arc internally.
    }
}

// ────────────────────────────────────────────────────────────────────────
// PyAsyncClient — async Python-facing client
// ────────────────────────────────────────────────────────────────────────

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct PyAsyncClient {
    inner: Client,
}

#[pymethods]
impl PyAsyncClient {
    #[new]
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, base_url=None, auth_bearer=None, transport=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<&Bound<'_, PyAny>>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        base_url: Option<&str>,
        auth_bearer: Option<String>,
        transport: Option<PyRef<'_, AsyncHTTPTransport>>,
    ) -> PyResult<Self> {
        let timeout_secs = PyTimeout::resolve_request_timeout(timeout, DEFAULT_TIMEOUT)?;
        let follow = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let max_r = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        let parsed_base_url = base_url.map(parse_base_url).transpose()?;

        if transport.is_some() && (verify.is_some() || cert.is_some() || timeout.is_some()) {
            return Err(RqxError::new_err(
                "Cannot specify both transport= and cert=/verify=/timeout=; pass options through one or the other".to_string(),
            ));
        }

        let transport_inner = match transport {
            Some(t) => t.inner.clone(),
            None => AsyncHTTPTransport::new(verify, cert, timeout)?.inner,
        };

        Ok(Self {
            inner: Client::new(
                transport_inner,
                timeout_secs,
                follow,
                max_r,
                parsed_base_url,
                auth_bearer,
            ),
        })
    }

    #[getter]
    fn base_url(&self) -> Option<String> {
        self.inner.base_url()
    }

    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.inner.cookies_snapshot()
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn request<'a>(
        &self,
        py: Python<'a>,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let method = method.to_string();
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .request(
                    &method,
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn get<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .get(
                    &url,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn options<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .options(
                    &url,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn head<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .head(
                    &url,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn delete<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .delete(
                    &url,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn post<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .post(
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn put<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .put(
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn patch<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .patch(
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn stream<'a>(
        &self,
        py: Python<'a>,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<QueryParams>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let method = method.to_string();
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let (response, elapsed) = inner
                .stream(
                    &method,
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    auth_bearer,
                    follow_redirects,
                    t,
                )
                .await?;
            let mut resp = PyAsyncStreamResponse::from_response(response)?;
            resp.parts.elapsed = elapsed;
            Ok(resp)
        })
    }

    fn __aenter__<'py>(slf: Py<Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(slf) })
    }

    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(false) })
    }
}

// ────────────────────────────────────────────────────────────────────────
// Shared sync helper: detach GIL, enter runtime, block on async future.
// ────────────────────────────────────────────────────────────────────────

fn block_on_inner<F, T>(py: Python<'_>, fut: F) -> PyResult<T>
where
    F: std::future::Future<Output = PyResult<T>> + Send,
    T: Send,
{
    py.detach(|| {
        RUNTIME
            .get()
            .ok_or_else(|| RqxError::new_err("runtime not initialized"))?
            .block_on(fut)
    })
}
