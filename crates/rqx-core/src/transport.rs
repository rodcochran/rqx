use reqwest::tls::Identity;
use reqwest::{Client, ClientBuilder, Request, Response};

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::error::*;
use crate::http::protocol::HttpVersionConfig;
use crate::http::tls::VerifyConfig;
use crate::request::RequestSpec;
use crate::response::PendingResponse;
use crate::retry::{FailureKind, Retry, RetryCounts};

#[derive(Clone)]
pub struct Transport {
    client: Client,
    semaphore: Option<Arc<Semaphore>>,
    pub retries: Option<Retry>,
}

impl Transport {
    pub fn new(client: Client, semaphore: Option<Arc<Semaphore>>, retries: Option<Retry>) -> Self {
        Self {
            client,
            semaphore,
            retries,
        }
    }

    /// Send with retries, body unread. Every path — buffered, redirect hops,
    /// streaming — goes through here so retries can't be skipped (https://github.com/rodcochran/rqx/issues/148).
    pub async fn send(&self, spec: &RequestSpec) -> Result<PendingResponse, RqxError> {
        if self.retries.is_some() {
            self.send_with_retries(spec).await
        } else {
            Ok(PendingResponse::new(self.send_raw(spec.clone_request()?).await?))
        }
    }

    /// Single attempt, no retries.
    async fn send_raw(&self, request: Request) -> Result<Response, RqxError> {
        self.execute(request).await.map_err(RqxError::from)
    }

    /// Error left unmapped so the retry loop can classify it.
    async fn execute(&self, request: Request) -> Result<Response, reqwest::Error> {
        let _permit = match self.semaphore.as_ref() {
            // acquire only fails on a closed semaphore; ours is never closed.
            Some(sem) => Some(
                sem.acquire()
                    .await
                    .expect("connection semaphore is never closed"),
            ),
            None => None,
        };
        self.client.execute(request).await
    }

    /// The retry state machine.
    async fn send_with_retries(&self, spec: &RequestSpec) -> Result<PendingResponse, RqxError> {
        // Operates on raw reqwest::Response throughout — reading status and
        // retry-after directly from response headers without acquiring the GIL.
        // The body stays unread for the caller. Mirrors the redirect-loop fix from https://github.com/rodcochran/rqx/issues/93.
        let r = self.retries.as_ref().unwrap();
        let is_retryable_method = r.allowed_methods.contains(spec.method().as_str());
        let backoff_max: f32 = r.backoff_max;
        let respect_retry = r.respect_retry_after_header;
        let total_timeout: f64 = r.total_timeout.unwrap_or(f64::INFINITY);

        let mut used = RetryCounts::default();
        let mut retry_history: Vec<(
            String,
            f64,
        )> = Vec::new();
        let mut current_response: Option<Response> = None;

        let start_time = Instant::now();

        loop {
            let attempt = used.total;

            if start_time.elapsed().as_secs_f64() > total_timeout {
                return Err(
                    MaxRetriesExceeded::new_err(
                        format!(
                            "total timeout of {}s exceeded after {} retries",
                            total_timeout, attempt,
                        ),
                    ),
                );
            }

            if attempt > 0 {
                let retry_after: f32 = if respect_retry {
                    current_response
                        .as_ref()
                        .and_then(
                            |resp| {
                                resp.headers()
                                    .get("retry-after")
                                    .and_then(|v| v.to_str().ok())
                                    .map(String::from)
                            },
                        )
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
                let backoff_time = f32::min(
                    f32::max(
                        calculated_backoff,
                        retry_after,
                    ),
                    backoff_max,
                );

                tokio::time::sleep(Duration::from_secs_f32(backoff_time)).await
            }

            // Drain the previous attempt's body to release the connection back
            // to the pool before issuing the next request. Required because we
            // hold raw Response across iterations; PyResponse wrapping would
            // have drained implicitly via response.bytes().await.
            if let Some(old) = current_response.take() {
                let _ = old.bytes().await;
            }

            let attempt_start = Instant::now();
            let failure = match self.execute(spec.clone_request()?).await {
                Ok(resp) => {
                    if !is_retryable_method {
                        return Ok(PendingResponse::new(resp));
                    }

                    let status = resp.status().as_u16();
                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    if attempt > 0 {
                        retry_history.push((
                            status.to_string(),
                            attempt_elapsed,
                        ));
                    }

                    if !r.status_forcelist.contains(&status) {
                        return Ok(
                            PendingResponse::new(resp).with_retries(
                                used.total as u32,
                                retry_history,
                            ),
                        );
                    }

                    current_response = Some(resp);
                    FailureKind::Status
                }
                Err(e) => {
                    let kind = FailureKind::from_request_error(&e);
                    let err = RqxError::from(e);
                    if !is_retryable_method {
                        return Err(err);
                    }
                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    if attempt > 0 {
                        retry_history.push((
                            format!(
                                "{}",
                                err
                            ),
                            attempt_elapsed,
                        ));
                    }
                    current_response = None;
                    kind
                }
            };

            if !r.allows_another(
                failure, &used,
            ) {
                break;
            }
            used.record(failure);
        }

        let exhausted = format!(
            "max retries exceeded after {} retries ({} connect, {} read, {} status)",
            used.total, used.connect, used.read, used.status
        );
        match current_response {
            Some(cr) => {
                // When status_forcelist matched and retries were exhausted:
                // raise_on_status=true (default) → raise MaxRetriesExceeded
                // raise_on_status=false → return the failing response so the
                //   caller can inspect status_code / headers / body.
                let status = cr.status().as_u16();
                if r.status_forcelist.contains(&status) && r.raise_on_status {
                    return Err(MaxRetriesExceeded::new_err(exhausted));
                }
                Ok(
                    PendingResponse::new(cr).with_retries(
                        used.total as u32,
                        retry_history,
                    ),
                )
            }
            None => Err(MaxRetriesExceeded::new_err(exhausted)),
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl Default for Transport {
    fn default() -> Self {
        let client = build_http_client(
            None, None, None, None, None, None, None, None,
        )
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
