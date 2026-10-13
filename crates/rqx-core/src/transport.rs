use http::StatusCode;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::tls::Identity;
use tokio::sync::Semaphore;

use crate::error::*;
use crate::http::protocol::HttpVersionConfig;
use crate::http::tls::VerifyConfig;
use crate::response::PendingResponse;
use crate::retry::{FailureKind, Retry, RetryCounts};
use crate::timeout::Timeout;

#[derive(Clone, Default)]
pub struct ConnectionPoolConfig {
    max_connections: Option<u32>,
    max_keepalive: Option<u32>,
    keepalive_expiry: Option<f64>,
}

impl ConnectionPoolConfig {
    pub fn new(
        max_connections: Option<u32>,
        max_keepalive: Option<u32>,
        keepalive_expiry: Option<f64>,
    ) -> Self {
        Self {
            max_connections,
            max_keepalive,
            keepalive_expiry,
        }
    }
}

#[derive(Clone, Default)]
pub struct TransportConfig {
    pool_config: ConnectionPoolConfig,
    http_version_config: HttpVersionConfig,
    verify_config: VerifyConfig,
    timeout_config: Timeout,
    pub retry_config: Retry,
    cert: Option<Identity>,
    proxies: Vec<reqwest::Proxy>,
}

impl TransportConfig {
    pub fn new(
        pool_config: ConnectionPoolConfig,
        http_version_config: HttpVersionConfig,
        verify_config: VerifyConfig,
        timeout_config: Timeout,
        retry_config: Retry,
        cert: Option<Identity>,
        proxies: Vec<reqwest::Proxy>,
    ) -> Self {
        Self {
            pool_config,
            http_version_config,
            verify_config,
            timeout_config,
            retry_config,
            cert,
            proxies,
        }
    }
}

impl From<&TransportConfig> for reqwest::ClientBuilder {
    fn from(value: &TransportConfig) -> Self {
        let mut client_builder = reqwest::Client::builder();

        if let Some(max_keepalive) = value.pool_config.max_keepalive {
            client_builder = client_builder.pool_max_idle_per_host(max_keepalive as usize);
        };
        if let Some(p) = value
            .pool_config
            .keepalive_expiry
            .or(value.timeout_config.pool)
        {
            client_builder = client_builder.pool_idle_timeout(Duration::from_secs_f64(p));
        };

        if let Some(c) = value.timeout_config.connect {
            client_builder = client_builder.connect_timeout(Duration::from_secs_f64(c));
        };
        if let Some(r) = value.timeout_config.read {
            client_builder = client_builder.read_timeout(Duration::from_secs_f64(r));
        };

        match value.http_version_config {
            HttpVersionConfig::Negotiate => {
                // No-op — reqwest's default does ALPN negotiation over TLS.
            }
            HttpVersionConfig::Http1Only => {
                client_builder = client_builder.http1_only();
            }
            HttpVersionConfig::Http2Only => {
                client_builder = client_builder.http2_prior_knowledge();
            }
        };

        match &value.verify_config {
            VerifyConfig::Default => {}
            VerifyConfig::DisableVerification => {
                client_builder = client_builder.danger_accept_invalid_certs(true);
            }
            VerifyConfig::CustomCa(ca) => {
                // TODO: add_root_certificate() is deprecated...
                client_builder = client_builder.add_root_certificate(ca.clone());
            }
        };

        if let Some(c) = &value.cert {
            client_builder = client_builder.identity(c.clone());
        };

        for p in &value.proxies {
            client_builder = client_builder.proxy(p.clone());
        }

        // Turn off reqwest's redirect handler since we do it with more granular control
        // in Client.
        client_builder = client_builder.redirect(reqwest::redirect::Policy::none());
        client_builder = client_builder.cookie_store(true);
        client_builder
    }
}

#[derive(Clone)]
pub struct Transport {
    pub client: reqwest::Client,
    pub config: TransportConfig,
    semaphore: Option<Arc<Semaphore>>,
}

impl Default for Transport {
    fn default() -> Self {
        let config = TransportConfig::default();
        Self::new(config).expect("Failed to build HTTP client")
    }
}

impl Transport {
    pub fn new(config: TransportConfig) -> Result<Self, RqxCoreError> {
        let semaphore = config
            .pool_config
            .max_connections
            .map(|mc| Arc::new(Semaphore::new(mc as usize)));

        let client_builder = reqwest::ClientBuilder::from(&config);
        let client = client_builder.build()?;

        Ok(Self {
            client,
            semaphore,
            config,
        })
    }

    pub async fn send(&self, request: reqwest::Request) -> Result<PendingResponse, RqxCoreError> {
        if self.config.retry_config.total > 0 {
            self.send_with_retries(request).await
        } else {
            // fast-path, if total number of allowed retries is 0, then just go here.
            Ok(PendingResponse::new(self.send_raw(request).await?))
        }
    }

    /// Single attempt, no retries.
    async fn send_raw(&self, request: reqwest::Request) -> Result<reqwest::Response, RqxCoreError> {
        self.execute(request).await.map_err(RqxCoreError::from)
    }

    /// Error left unmapped so the retry loop can classify it.
    async fn execute(
        &self,
        request: reqwest::Request,
    ) -> Result<reqwest::Response, reqwest::Error> {
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
    async fn send_with_retries(
        &self,
        request: reqwest::Request,
    ) -> Result<PendingResponse, RqxCoreError> {
        // Operates on raw reqwest::Response throughout — reading status and
        // retry-after directly from response headers without acquiring the GIL.
        // The body stays unread for the caller. Mirrors the redirect-loop fix from https://github.com/rodcochran/rqx/issues/93.
        let is_retryable_method = self
            .config
            .retry_config
            .allowed_methods
            .contains(request.method().as_str());
        let total_timeout: f64 = self
            .config
            .retry_config
            .total_timeout
            .unwrap_or(f64::INFINITY);

        let mut retry_counts = RetryCounts::default();
        // TODO: is retry_history lost after a single request?
        // CASE: if a single reuqest is executed with retries, then redirectd, then retried again? is that intentional?
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        let mut current_response: Option<reqwest::Response> = None;

        let start_time = Instant::now();

        for attempt_num in 0..=self.config.retry_config.total {
            // Exit for timeout
            if start_time.elapsed().as_secs_f64() > total_timeout {
                // TODO: determine if we want to raise MaxRetriesExceeded or a timeout error...
                return Err(RqxCoreError::MaxRetriesExceeded(format!(
                    "total timeout of {}s exceeded after {} retries",
                    total_timeout, attempt_num,
                )));
            }

            let new_request = request.try_clone().ok_or_else(|| {
                RqxCoreError::RequestError(
                    "Streaming request bodies cannot be replayed".to_string(),
                )
            })?;

            let attempt = self.send_attempt(new_request).await;

            // Here we have to be at a failure because any non-retryable success will be returned
            let failure = match attempt.kind {
                AttemptKind::Success(successful_attempt) => {
                    // Early exit if the method isn't retryable.
                    if !is_retryable_method {
                        return Ok(PendingResponse::new(successful_attempt.response)
                            .with_retries(retry_counts.total as u32, retry_history));
                    }
                    // Early exit for success.
                    // Here, success doesn't mean a 2XX response. It means that the request went through.
                    if !successful_attempt
                        .status_is_retryable(&self.config.retry_config.status_forcelist)
                    {
                        return Ok(PendingResponse::new(successful_attempt.response)
                            .with_retries(retry_counts.total as u32, retry_history));
                    }
                    current_response = Some(successful_attempt.response);
                    FailureKind::Status
                }
                AttemptKind::Failure(failed_attempt) => {
                    // Early exit if the method isn't retryable.
                    if !is_retryable_method {
                        return Err(failed_attempt.err);
                    }
                    current_response = None;
                    failed_attempt.kind
                }
            };

            // indicates we're in a retry
            if attempt_num > 0 {
                retry_history.push((attempt.message, attempt.elapsed));
            };

            // Add the failure to the retry counts.
            retry_counts.record(failure);

            // Out of retries.
            if !self.config.retry_config.allows_another(&retry_counts) {
                break;
            }
            // Apply backoff
            self.apply_backoff(current_response.as_ref(), attempt_num + 1)
                .await;
        }

        let exhausted = format!(
            "max retries exceeded after {} retries ({} connect, {} read, {} status)",
            retry_counts.total, retry_counts.connect, retry_counts.read, retry_counts.status
        );
        match current_response {
            Some(cr) => {
                // When status_forcelist matched and retries were exhausted:
                // raise_on_status=true (default) → raise MaxRetriesExceeded
                // raise_on_status=false → return the failing response so the
                //   caller can inspect status_code / headers / body.
                let status = cr.status().as_u16();
                if self.config.retry_config.raise_on_status
                    && self.config.retry_config.status_forcelist.contains(&status)
                {
                    return Err(RqxCoreError::MaxRetriesExceeded(exhausted));
                }
                Ok(PendingResponse::new(cr).with_retries(retry_counts.total as u32, retry_history))
            }
            None => Err(RqxCoreError::MaxRetriesExceeded(exhausted)),
        }
    }

    async fn send_attempt(&self, request: reqwest::Request) -> Attempt {
        // Goal of this function is to extract metadata, not mutate any of the retry body machine stuff.
        // leave that up to the send_with_retries to handle.
        let attempt_start = Instant::now();
        match self.execute(request).await {
            Ok(resp) => {
                let msg = resp.status().to_string();
                Attempt {
                    kind: AttemptKind::Success(SuccessfulAttempt {
                        status: resp.status(),
                        response: resp,
                    }),
                    elapsed: attempt_start.elapsed().as_millis() as f64,
                    message: msg,
                }
            }
            Err(e) => {
                let failure_kind = FailureKind::from_request_error(&e);
                let status = e.status();
                let rqx_error = RqxCoreError::from(e);
                let message = format!("{}", rqx_error.to_string());
                Attempt {
                    kind: AttemptKind::Failure(FailedAttempt {
                        kind: failure_kind,
                        status,
                        err: rqx_error,
                    }),
                    elapsed: attempt_start.elapsed().as_millis() as f64,
                    message,
                }
            }
        }
    }

    async fn apply_backoff(
        &self,
        current_response: Option<&reqwest::Response>,
        attempt_number: i32,
    ) {
        let respect_retry = self.config.retry_config.respect_retry_after_header;
        let backoff_max: f32 = self.config.retry_config.backoff_max;

        let retry_after: f32 = if respect_retry {
            self.get_retry_after_from_reqwest_response(current_response)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let backoff_value = self
            .config
            .retry_config
            .calculate_backoff_for_attempt(attempt_number);
        let backoff_time = f32::min(f32::max(backoff_value, retry_after), backoff_max);
        if backoff_time > 0.0 {
            tokio::time::sleep(Duration::from_secs_f32(backoff_time)).await
        }
    }

    fn get_retry_after_from_reqwest_response(
        &self,
        resp: Option<&reqwest::Response>,
    ) -> Option<f32> {
        resp.and_then(|resp| {
            resp.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        })
        .and_then(|v| v.parse::<f32>().ok())
    }
}

struct SuccessfulAttempt {
    status: StatusCode,
    response: reqwest::Response,
}

impl SuccessfulAttempt {
    fn status_is_retryable(&self, status_forcelist: &HashSet<u16>) -> bool {
        status_forcelist.contains(&self.status.as_u16())
    }
}

struct FailedAttempt {
    kind: FailureKind,
    // Failures can happen on the network or in the reqwest core code.
    // A non-network failure will give us None for status.
    #[warn(unused)]
    status: Option<StatusCode>,
    err: RqxCoreError,
}

enum AttemptKind {
    Success(SuccessfulAttempt),
    Failure(FailedAttempt),
}

struct Attempt {
    kind: AttemptKind,
    elapsed: f64,
    message: String,
}
