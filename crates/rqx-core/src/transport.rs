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
    pool_timeout: Option<f64>,
}

impl ConnectionPoolConfig {
    pub fn new(
        max_connections: Option<u32>,
        max_keepalive: Option<u32>,
        keepalive_expiry: Option<f64>,
        pool_timeout: Option<f64>,
    ) -> Self {
        Self {
            max_connections,
            max_keepalive,
            keepalive_expiry,
            pool_timeout,
        }
    }
}

#[derive(Clone, Default)]
pub struct TransportConfig {
    pool_config: ConnectionPoolConfig,
    http_version_config: HttpVersionConfig,
    verify_config: VerifyConfig,
    timeout_config: Timeout,
    retry_config: Option<Retry>,
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
            .or(value.pool_config.pool_timeout)
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

        client_builder
    }
}

#[derive(Clone)]
pub struct Transport {
    pub client: reqwest::Client,
    config: TransportConfig,
    semaphore: Option<Arc<Semaphore>>,
}

impl Transport {
    pub fn new(config: TransportConfig) -> Result<Self, RqxError> {
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

    pub async fn send(&self, request: reqwest::Request) -> Result<PendingResponse, RqxError> {
        if self.config.retry_config.is_some() {
            self.send_with_retries(request).await
        } else {
            Ok(PendingResponse::new(self.send_raw(request).await?))
        }
    }

    /// Single attempt, no retries.
    async fn send_raw(&self, request: reqwest::Request) -> Result<reqwest::Response, RqxError> {
        self.execute(request).await.map_err(RqxError::from)
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
    ) -> Result<PendingResponse, RqxError> {
        // Operates on raw reqwest::Response throughout — reading status and
        // retry-after directly from response headers without acquiring the GIL.
        // The body stays unread for the caller. Mirrors the redirect-loop fix from https://github.com/rodcochran/rqx/issues/93.
        let r = self.config.retry_config.as_ref().unwrap();
        let is_retryable_method = r.allowed_methods.contains(request.method().as_str());
        let backoff_max: f32 = r.backoff_max;
        let respect_retry = r.respect_retry_after_header;
        let total_timeout: f64 = r.total_timeout.unwrap_or(f64::INFINITY);

        let mut used = RetryCounts::default();
        let mut retry_history: Vec<(String, f64)> = Vec::new();
        let mut current_response: Option<reqwest::Response> = None;

        let start_time = Instant::now();

        loop {
            let attempt = used.total;

            if start_time.elapsed().as_secs_f64() > total_timeout {
                return Err(HTTPError::MaxRetriesExceeded(format!(
                    "total timeout of {}s exceeded after {} retries",
                    total_timeout, attempt,
                ))
                .into());
            }

            if attempt > 0 {
                let retry_after: f32 = if respect_retry {
                    current_response
                        .as_ref()
                        .and_then(|resp| {
                            resp.headers()
                                .get("retry-after")
                                .and_then(|v| v.to_str().ok())
                                .map(String::from)
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

            let new_request = request.try_clone().ok_or_else(|| {
                RequestError::RequestError(
                    "Streaming request bodies cannot be replayed".to_string(),
                )
            })?;

            let failure = match self.execute(new_request).await {
                Ok(resp) => {
                    if !is_retryable_method {
                        return Ok(PendingResponse::new(resp));
                    }

                    let status = resp.status().as_u16();
                    let attempt_elapsed = attempt_start.elapsed().as_millis() as f64;
                    if attempt > 0 {
                        retry_history.push((status.to_string(), attempt_elapsed));
                    }

                    if !r.status_forcelist.contains(&status) {
                        return Ok(PendingResponse::new(resp)
                            .with_retries(used.total as u32, retry_history));
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
                        retry_history.push((format!("{}", err), attempt_elapsed));
                    }
                    current_response = None;
                    kind
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
                    return Err(HTTPError::MaxRetriesExceeded(exhausted).into());
                }
                Ok(PendingResponse::new(cr).with_retries(used.total as u32, retry_history))
            }
            None => Err(HTTPError::MaxRetriesExceeded(exhausted).into()),
        }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }
}
