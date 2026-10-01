use std::collections::HashMap;
use std::sync::Arc;

use std::time::{Duration, Instant};
use tokio::sync::Mutex as TokioMutex;
use url::Url;

use crate::auth::Auth;
use crate::error::*;

use crate::redirect::{Redirect, RedirectPolicy};
use crate::request::Request;
use crate::request_components::body::RequestBody;
use crate::response::{BufferedResponse, PendingResponse};
use crate::streaming::context::Unsent;
use crate::timeout::Timeout;
use crate::transport::Transport;
use crate::url::base_url::BaseUrl;
use crate::url::reference::UrlReference;

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

    pub fn redirects(&self) -> RedirectPolicy {
        self.config.redirects
    }

    /// Build and send a request, then buffer the body.
    pub async fn request(&self, request: Request) -> Result<BufferedResponse, RqxError> {
        let follow_redirects = request
            .follow_redirects
            .unwrap_or(self.config.redirects.follow);
        let executable_request = self.build(request)?;
        self.send(executable_request, follow_redirects)
            .await?
            .read()
            .await
    }

    /// Send a built request, leaving the body unread for the stream response
    /// to consume. `elapsed` is the time to headers.
    pub fn stream(&self, request: Request) -> Result<Unsent, RqxError> {
        let follow_redirects = request
            .follow_redirects
            .unwrap_or(self.config.redirects.follow);
        let executable_request = self.build(request)?;
        Ok(Unsent::new(
            self.clone(),
            executable_request,
            follow_redirects,
        ))
    }

    pub fn build(&self, request: Request) -> Result<reqwest::Request, RqxError> {
        let mut url = self.merge_url(request.url)?;

        if !matches!(url.scheme(), "http" | "https") {
            return Err(TransportError::UnsupportedProtocol(format!(
                "Request URL has an unsupported protocol '{}://'.",
                url.scheme()
            ))
            .into());
        }

        // Set query params if they exist and are populated.
        if let Some(params) = &request.params {
            let query = params.to_string();
            match query.is_empty() {
                true => url.set_query(None),
                false => url.set_query(Some(&query)),
            }
        };

        // Initialize reqwest's RequestBuilder
        let mut builder = self.transport.client.request(request.method.clone(), url);

        // Apply headers if they exist
        if let Some(headers) = &request.headers {
            builder = builder.headers(headers.inner.clone());
        };

        // Use current requests auth override, or client default.
        // Non-None override -> use Request's Auth.
        // If Requests, auth is explicitly Auth::None, this request uses no Auth.
        // Empty override -> client default.
        match request.auth.as_ref().unwrap_or(&self.config.auth) {
            Auth::None => {}
            Auth::Basic { username, password } => {
                builder = builder.basic_auth(username, Some(password));
            }
            Auth::Bearer(token) => {
                builder = builder.bearer_auth(token);
            }
        };

        match &request.body {
            RequestBody::Content(c) => {
                builder = builder.body(c.clone());
            }
            RequestBody::Form(f) => {
                builder = builder.form(f);
            }
            RequestBody::Json(j) => {
                builder = builder.json(j);
            }
            RequestBody::Empty => {}
        };

        if let Some(timeout) = request.timeout {
            builder = builder.timeout(Duration::from_secs_f64(timeout))
        }

        let executable_request = builder.build().map_err(RqxError::from)?;
        Ok(executable_request)
    }

    fn merge_url(&self, url: UrlReference) -> Result<Url, RqxError> {
        match (url, &self.config.base_url) {
            (UrlReference::Absolute(absolute), _) if absolute.has_authority() => Ok(absolute),
            (url, Some(base)) => base.join(&url),
            (_, None) => Err(TransportError::UnsupportedProtocol(
                "Request URL is missing an 'http://' or 'https://' protocol.".to_string(),
            )
            .into()),
        }
    }

    /// Send a built request — following redirects when asked — and accumulate
    /// the final response's cookies. Shared by `request` and `stream`.
    pub(crate) async fn send(
        &self,
        request: reqwest::Request,
        follow_redirects: bool,
    ) -> Result<PendingResponse, RqxError> {
        let start_time = Instant::now();
        let mut pending = if follow_redirects {
            self.follow_redirects(request).await?
        } else {
            self.transport.send(request).await?
        };
        self.accumulate_cookies(&pending.parts.cookies).await;
        pending.parts.elapsed = start_time.elapsed();
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
    async fn follow_redirects(
        &self,
        mut request: reqwest::Request,
    ) -> Result<PendingResponse, RqxError> {
        let mut redirects_used: u32 = 0;
        let mut num_retries: u32 = 0;
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        loop {
            let outgoing_request = request.try_clone().ok_or_else(|| {
                RequestError::RequestError("Request body cannot be replayed".to_string())
            })?;
            let mut hop = self.transport.send(outgoing_request).await?;
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
            let new_url = Redirect::redirect_target(&request.url(), &location)?;
            request = Redirect::redirected_request(request, status, new_url);
            redirects_used += 1;
        }
    }
}
