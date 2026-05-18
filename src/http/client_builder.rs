use pyo3::Bound;
use pyo3::prelude::PyResult;
use pyo3::types::PyAny;
use reqwest::tls::Identity;
use reqwest::{Client, ClientBuilder};
use std::collections::HashMap;
use std::time::Duration;

use crate::http::protocol::HttpVersionConfig;
use crate::http::proxy::parse_proxies;
use crate::http::tls::{VerifyConfig, parse_identity};
use crate::timeout::PyTimeout;

/// Wraps reqwest's ClientBuilder with rqx's configuration vocabulary.
///
/// Methods are sliced by *what they configure on reqwest*, not by *which
/// Python argument they came from* — so each concern owns exactly one
/// set of reqwest setters and there are no inter-method collisions.
///
/// All `with_*` methods consume and return `Self` to support chaining.
pub struct RqxClientBuilder {
    inner: ClientBuilder,
}

impl Default for RqxClientBuilder {
    fn default() -> Self {
        todo!()
    }
}

impl RqxClientBuilder {
    /// New builder seeded with rqx's baseline:
    /// - `redirect::Policy::none()` (PyClient layer handles redirects)
    /// - `cookie_store(true)`
    pub fn new() -> Self {
        let http_client_builder = Client::builder()
            // Explicitly add no redirects at the transport level, as we let the PyClient take care of it
            .redirect(reqwest::redirect::Policy::none())
            .cookie_store(true);

        Self {
            inner: http_client_builder,
        }
    }

    /// Configures the connection pool. Owns every `pool_*` setter on reqwest.
    ///
    /// Resolves the precedence between `keepalive_expiry` and `timeout.pool`
    /// (caller passes the latter as `pool_timeout` — `keepalive_expiry`
    /// wins when both are set).
    pub fn with_pool(
        mut self,
        max_keepalive: Option<u32>,
        keepalive_expiry: Option<f64>,
        pool_timeout: Option<f64>,
    ) -> Self {
        if let Some(max_keepalive) = max_keepalive {
            self.inner = self.inner.pool_max_idle_per_host(max_keepalive as usize);
        }

        if let Some(ke) = keepalive_expiry {
            self.inner = self.inner.pool_idle_timeout(Duration::from_secs_f64(ke));
        }

        if let Some(p) = pool_timeout {
            // Only set if keepalive_expiry didn't already.
            if keepalive_expiry.is_none() {
                self.inner = self.inner.pool_idle_timeout(Duration::from_secs_f64(p));
            }
        }

        return self;
    }

    pub fn with_phase_timeouts(mut self, connect: Option<f64>, read: Option<f64>) -> Self {
        if let Some(c) = connect {
            self.inner = self.inner.connect_timeout(Duration::from_secs_f64(c));
        }
        if let Some(r) = read {
            self.inner = self.inner.read_timeout(Duration::from_secs_f64(r));
        }
        return self;
    }

    /// HTTP version selection. Takes a pre-validated [`HttpVersionConfig`];
    /// the (false, false) error case is caught upstream in `from_args`.
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

        return self;
    }

    /// Proxy configuration. Takes pre-parsed `reqwest::Proxy` values;
    /// URL parsing and scheme filtering happen upstream in `parse_proxies`.
    pub fn with_proxy(mut self, proxies: Vec<reqwest::Proxy>) -> Self {
        for p in proxies {
            self.inner = self.inner.proxy(p);
        }
        self
    }

    /// Finalize into a reqwest `Client`. Panics if reqwest's build fails
    /// (matches the original behavior — failure here indicates a logic
    /// error in the builder chain, not user input).
    pub fn build(self) -> Client {
        self.inner.build().expect("Failed to build HTTP client")
    }
}

/*

Helper for constructing the HTTP Client

*/

pub fn build_http_client(
    max_keepalive_connections: Option<u32>,
    keepalive_expiry: Option<f64>,
    http1: Option<bool>,
    http2: Option<bool>,
    verify: Option<&Bound<'_, PyAny>>,
    cert: Option<&Bound<'_, PyAny>>,
    proxy: Option<HashMap<String, String>>,
    timeout: Option<&Bound<'_, PyAny>>,
) -> PyResult<Client> {
    let (connect_timeout, read_timeout, pool_timeout) = match timeout {
        Some(t) => {
            let parsed = PyTimeout::extract_any(t)?;
            (parsed.connect, parsed.read, parsed.pool)
        }
        None => (None, None, None),
    };

    let verify_cfg = verify.map(VerifyConfig::from_py_any).transpose()?;
    let identity = cert.map(parse_identity).transpose()?;
    let http_version = HttpVersionConfig::from_args(http1, http2)?;
    let proxies = parse_proxies(proxy)?;

    let client = RqxClientBuilder::new()
        .with_pool(max_keepalive_connections, keepalive_expiry, pool_timeout)
        .with_http_version(http_version)
        .with_phase_timeouts(connect_timeout, read_timeout)
        .with_proxy(proxies)
        .with_tls(verify_cfg, identity)
        .build();

    Ok(client)
}
