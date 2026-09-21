use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex as TokioMutex;
use url::Url;

use crate::error::*;
use crate::headers::Headers;
use crate::query_params::QueryPairs;
use crate::request::{RequestBody, RequestSpec};
use crate::response::{BufferedResponse, PendingResponse};
use crate::retry::DEFAULT_RAISE_ON_REDIRECT;
use crate::transport::Transport;
use crate::url::reference::UrlReference;
use crate::url::request_url::BaseUrl;
use crate::url::url::RqxClientUrl;

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
    base_url: Option<BaseUrl>,
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
        base_url: Option<BaseUrl>,
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

    pub fn base_url(&self) -> Option<&BaseUrl> {
        self.base_url.as_ref()
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

    /// Build and send a request, then buffer the body.
    pub async fn request(
        &self,
        method: &str,
        url: RqxClientUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
        let request = self.build(
            method,
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            auth_bearer,
            timeout,
        )?;
        // `stream` stamps `elapsed` when the headers arrive; reading the body doesn't move it.
        self.stream(request, follow_redirects).await?.read().await
    }

    /// Send a built request, leaving the body unread for the stream response
    /// to consume. `elapsed` is the time to headers.
    pub async fn stream(
        &self,
        request: RequestSpec,
        follow_redirects: Option<bool>,
    ) -> Result<PendingResponse, RqxError> {
        let start_time = Instant::now();
        let mut pending = self.send(request, follow_redirects).await?;
        pending.parts.elapsed = start_time.elapsed();
        Ok(pending)
    }

    pub fn build(
        &self,
        method: &str,
        url: RqxClientUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        timeout: f64,
    ) -> Result<RequestSpec, RqxError> {
        // Resolve bearer: per-request override wins; otherwise fall back to
        // the client-level default. Then enforce the basic-vs-bearer collision
        // rule against the effective values that would actually be applied.
        let bearer = auth_bearer.or_else(|| self.auth_bearer.clone());
        if auth.is_some() && bearer.is_some() {
            return Err(RequestError::RequestError(
                "Cannot specify both auth= (basic) and auth_bearer= on the same request"
                    .to_string(),
            )
            .into());
        }

        RequestSpec::build(
            self.transport.client(),
            method,
            self.merge_url(&url)?,
            params,
            RequestBody::new(content, data, json)?,
            headers,
            auth,
            bearer.as_deref(),
            timeout,
        )
    }

    fn merge_url(&self, url: &RqxClientUrl) -> Result<Url, RqxError> {
        if let UrlReference::Absolute(absolute) = url.get_inner()
            && absolute.has_authority()
        {
            return Ok(absolute);
        }
        match &self.base_url {
            Some(base) => base.join(url),
            None => Err(TransportError::UnsupportedProtocol(
                "Request URL is missing an 'http://' or 'https://' protocol.".to_string(),
            )
            .into()),
        }
    }

    /// Send a built request — following redirects when asked — and accumulate
    /// the final response's cookies. Shared by `request` and `stream`.
    async fn send(
        &self,
        spec: RequestSpec,
        follow_redirects: Option<bool>,
    ) -> Result<PendingResponse, RqxError> {
        let follow = follow_redirects.unwrap_or(self.follow_redirects);
        let pending = if follow {
            self.follow_redirects(spec).await?
        } else {
            self.transport.send(&spec).await?
        };
        self.accumulate_cookies(&pending.parts.cookies).await;
        Ok(pending)
    }

    pub async fn get(
        &self,
        url: RqxClientUrl,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
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
        url: RqxClientUrl,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
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
        url: RqxClientUrl,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
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
        url: RqxClientUrl,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
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
        url: RqxClientUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
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
        url: RqxClientUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
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
        url: RqxClientUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<serde_json::Value>,
        params: Option<QueryPairs>,
        headers: Option<Headers>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
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

    /// Follow the HTTP redirect chain. Returns the final hop with its body
    /// unread — intermediate-hop `Set-Cookie` headers are accumulated into
    /// `self.cookies` as a side effect.
    ///
    /// Each hop goes through `Transport::send`, so retries apply per hop and
    /// the telemetry on the final response adds up across the chain (https://github.com/rodcochran/rqx/issues/148).
    ///
    /// Reads status, Location, and Set-Cookie off `parts`, so no GIL
    /// acquisition per hop (see https://github.com/rodcochran/rqx/issues/93).
    async fn follow_redirects(&self, spec: RequestSpec) -> Result<PendingResponse, RqxError> {
        let raise_on_redirect = self
            .transport
            .retries
            .as_ref()
            .map(|r| r.raise_on_redirect)
            .unwrap_or(DEFAULT_RAISE_ON_REDIRECT);

        let mut current = spec;
        let mut redirects_used: u32 = 0;
        let mut num_retries: u32 = 0;
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        loop {
            let mut hop = self.transport.send(&current).await?;
            num_retries += hop.parts.num_retries;
            retry_history.append(&mut hop.parts.retry_history);
            let status = hop.parts.status_code;

            if !(300..400).contains(&status) {
                return Ok(hop.with_retries(num_retries, retry_history));
            }

            self.accumulate_cookies(&hop.parts.cookies).await;

            if redirects_used + 1 >= self.max_redirects {
                if raise_on_redirect {
                    return Err(RequestError::TooManyRedirects(format!(
                        "Exceeded max redirects {}",
                        self.max_redirects
                    ))
                    .into());
                }
                return Ok(hop.with_retries(num_retries, retry_history));
            }

            let location = hop
                .parts
                .headers
                .get_first("location")
                .map(String::from)
                .ok_or_else(|| {
                    ProtocolError::RemoteProtocolError(
                        "3xx response missing Location header".to_string(),
                    )
                })?;

            // Drain the 3xx body to release the connection back to the pool.
            hop.drain().await;

            // Resolve against the hop that sent the Location, not the original URL.
            let new_url = current.redirect_target(&location)?;
            current = current.redirected(status, new_url)?;

            redirects_used += 1;
        }
    }
}
