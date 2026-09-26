use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex as TokioMutex;
use url::Url;

use crate::auth::Auth;
use crate::error::*;
use crate::headers::Headers;
use crate::query_params::QueryPairs;
use crate::redirect::RedirectPolicy;
use crate::request::{RequestBody, RequestSpec};
use crate::response::{BufferedResponse, PendingResponse};

use crate::timeout::Timeout;
use crate::transport::Transport;
use crate::url::client_url::RqxClientUrl;
use crate::url::reference::UrlReference;
use crate::url::request_url::BaseUrl;

const DEFAULT_TIMEOUT: f64 = 15.0;

#[derive(Clone, Default)]
pub struct ClientConfig {
    timeout: Timeout,
    redirects: RedirectPolicy,
    base_url: Option<BaseUrl>,
    auth: Auth,
}

impl ClientConfig {
    pub fn new(
        timeout: Timeout,
        redirects: RedirectPolicy,
        base_url: Option<BaseUrl>,
        auth: Auth,
    ) -> Self {
        Self {
            timeout,
            redirects,
            base_url,
            auth,
        }
    }
}

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
    config: ClientConfig,
    cookies: Arc<TokioMutex<HashMap<String, String>>>,
}

impl Client {
    pub fn new(transport: Transport, config: ClientConfig) -> Self {
        Self {
            transport,
            config,
            cookies: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    pub fn base_url(&self) -> Option<&BaseUrl> {
        self.config.base_url.as_ref()
    }

    pub fn timeout_secs(&self) -> f64 {
        self.config
            .timeout
            .per_request_total()
            .unwrap_or(DEFAULT_TIMEOUT)
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
        auth: Option<Auth>,
        follow_redirects: Option<bool>,
        timeout: f64,
    ) -> Result<BufferedResponse, RqxError> {
        let request = self.build(
            method, url, content, data, json, params, headers, auth, timeout,
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
        auth: Option<Auth>,
        timeout: f64,
    ) -> Result<RequestSpec, RqxError> {
        RequestSpec::build(
            self.transport.client(),
            method,
            self.merge_url(&url)?,
            params,
            RequestBody::new(content, data, json)?,
            headers,
            auth.as_ref().unwrap_or(&self.config.auth),
            timeout,
        )
    }

    fn merge_url(&self, url: &RqxClientUrl) -> Result<Url, RqxError> {
        if let UrlReference::Absolute(absolute) = url.get_inner()
            && absolute.has_authority()
        {
            return Ok(absolute);
        }
        match &self.config.base_url {
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
        // TODO: wire redirect policy through the transport
        let follow = follow_redirects.unwrap_or(self.config.redirects.follow);
        let pending = if follow {
            self.follow_redirects(spec).await?
        } else {
            self.transport.send(&spec).await?
        };
        self.accumulate_cookies(&pending.parts.cookies).await;
        Ok(pending)
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
    async fn follow_redirects(&self, mut spec: RequestSpec) -> Result<PendingResponse, RqxError> {
        let mut redirects_used: u32 = 0;
        let mut num_retries: u32 = 0;
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        loop {
            let mut hop = self.transport.send(&spec).await?;
            num_retries += hop.parts.num_retries;
            retry_history.append(&mut hop.parts.retry_history);
            let status = hop.parts.status_code;

            if !(300..400).contains(&status) {
                return Ok(hop.with_retries(num_retries, retry_history));
            }

            self.accumulate_cookies(&hop.parts.cookies).await;

            if redirects_used + 1 >= self.config.redirects.max_redirects {
                if self.config.redirects.raise_on_exceeded {
                    return Err(RequestError::TooManyRedirects(format!(
                        "Exceeded max redirects {}",
                        self.config.redirects.max_redirects
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
            let new_url = spec.redirect_target(&location)?;
            spec = spec.redirected(status, new_url)?;

            redirects_used += 1;
        }
    }
}
