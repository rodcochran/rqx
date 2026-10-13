use http::StatusCode;
use serde_json::to_string;
use std::collections::HashSet;
use std::sync::Arc;
use std::thread::current;
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
    pub retry_config: Option<Retry>,
    cert: Option<Identity>,
    proxies: Vec<reqwest::Proxy>,
}

impl TransportConfig {
    pub fn new(
        pool_config: ConnectionPoolConfig,
        http_version_config: HttpVersionConfig,
        verify_config: VerifyConfig,
        timeout_config: Timeout,
        retry_config: Option<Retry>,
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
        if self.config.retry_config.is_some() {
            self.send_with_retries(request).await
        } else {
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
        // TODO: going to make the retry....unwrap() impossible.
        //
        // Operates on raw reqwest::Response throughout — reading status and
        // retry-after directly from response headers without acquiring the GIL.
        // The body stays unread for the caller. Mirrors the redirect-loop fix from https://github.com/rodcochran/rqx/issues/93.
        let r = self.config.retry_config.as_ref().unwrap();
        let is_retryable_method = r.allowed_methods.contains(request.method().as_str());
        let backoff_max: f32 = r.backoff_max;
        let respect_retry = r.respect_retry_after_header;
        let total_timeout: f64 = r.total_timeout.unwrap_or(f64::INFINITY);

        let mut used = RetryCounts::default();
        // TODO: is retry_history lost after a single request?
        // CASE: if a single reuqest is executed with retries, then redirectd, then retried again? is that intentional?
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        let mut current_response: Option<reqwest::Response> = None;

        let start_time = Instant::now();

        // gut feeling is that we shouldn't have anything after the loop.
        // The logic for exits should be fully contained in the loop?
        // loop logic:
        // for attempt_number in max_attempts:
        // - send_attempt
        // - add it to history
        // - return early for success
        // - if on last allowed attempt, return what we have or error.
        // - calculate backoff and continue

        loop {
            let attempt = used.total;

            if start_time.elapsed().as_secs_f64() > total_timeout {
                // TODO: determine if we want to raise MaxRetriesExceeded or a timeout error...
                return Err(RqxCoreError::MaxRetriesExceeded(format!(
                    "total timeout of {}s exceeded after {} retries",
                    total_timeout, attempt,
                )));
            }

            if attempt > 0 {
                let retry_after: f32 = if respect_retry {
                    self.get_retry_after_from_reqwest_response(current_response.as_ref(), 0.0)
                } else {
                    0.0
                };

                let backoff_value = r.calculate_backoff_for_attempt(attempt);
                let backoff_time = f32::min(f32::max(backoff_value, retry_after), backoff_max);

                tokio::time::sleep(Duration::from_secs_f32(backoff_time)).await
            }

            // Drain the previous attempt's body to release the connection back
            // to the pool before issuing the next request. Required because we
            // hold raw Response across iterations; PyResponse wrapping would
            // have drained implicitly via response.bytes().await.
            if let Some(old) = current_response.take() {
                let _ = old.bytes().await;
            }

            let new_request = request.try_clone().ok_or_else(|| {
                RqxCoreError::RequestError(
                    "Streaming request bodies cannot be replayed".to_string(),
                )
            })?;

            let latest_attempt = self.send_attempt(new_request).await;

            if attempt > 0 {
                retry_history.push((latest_attempt.message, latest_attempt.elapsed));
            };

            let failure = match latest_attempt.kind {
                AttemptKind::Success(successful_attempt) => {
                    if !successful_attempt.status_is_retryable(&r.status_forcelist) {
                        return Ok(PendingResponse::new(successful_attempt.response)
                            .with_retries(used.total as u32, retry_history));
                    }
                    current_response = Some(successful_attempt.response);
                    FailureKind::Status
                }
                AttemptKind::Failure(failed_attempt) => {
                    current_response = None;
                    failed_attempt.kind
                }
            };

            if !r.allows_another(failure, &used) {
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
                    return Err(RqxCoreError::MaxRetriesExceeded(exhausted));
                }
                Ok(PendingResponse::new(cr).with_retries(used.total as u32, retry_history))
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
            Err(e) => Attempt {
                kind: AttemptKind::Failure(FailedAttempt {
                    kind: FailureKind::from_request_error(&e),
                    status: e.status(),
                }),
                elapsed: attempt_start.elapsed().as_millis() as f64,
                message: format!("{}", RqxCoreError::from(e)),
            },
        }
    }

    fn get_retry_after_from_reqwest_response(
        &self,
        resp: Option<&reqwest::Response>,
        fallback_value: f32,
    ) -> f32 {
        resp.and_then(|resp| {
            resp.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        })
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(fallback_value)
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
    status: Option<StatusCode>,
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
