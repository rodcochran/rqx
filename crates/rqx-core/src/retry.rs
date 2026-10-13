use std::collections::HashSet;

const DEFAULT_TOTAL_RETRIES: i32 = 3;
const DEFAULT_BACKOFF_FACTOR: f32 = 0.0;
const DEFAULT_BACKOFF_MAX: f32 = 120.0;
const DEFAULT_BACKOFF_JITTER: f32 = 0.0;
const DEFAULT_STATUS_FORCELIST: &[u16] = &[];
const DEFAULT_ALLOWED_METHODS: &[&str] = &[
    "DELETE", "GET", "HEAD", "OPTIONS",
    "PUT",
    // "TRACE" // TODO: determine if we want to deal with TRACE methods...
];
const DEFAULT_RESPECT_RETRY_AFTER_HEADER: bool = true;
const DEFAULT_RAISE_ON_STATUS: bool = true;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailureKind {
    Connect,
    Read,
    Status,
}

impl FailureKind {
    pub fn from_request_error(e: &reqwest::Error) -> Self {
        if e.is_connect() {
            Self::Connect
        } else {
            Self::Read
        }
    }
}

// TODO: can we switch these to u32? dont think they could ever be less than 0...
#[derive(Default)]
pub struct RetryCounts {
    pub total: i32,
    pub connect: i32,
    pub read: i32,
    pub status: i32,
}

impl RetryCounts {
    pub fn record(&mut self, kind: FailureKind) {
        self.total += 1;
        match kind {
            FailureKind::Connect => self.connect += 1,
            FailureKind::Read => self.read += 1,
            FailureKind::Status => self.status += 1,
        }
    }
}

#[derive(Clone)]
pub struct Retry {
    pub total: i32,
    pub connect: i32,
    pub read: i32,
    pub status: i32,
    pub backoff_factor: f32,
    pub backoff_max: f32,
    pub backoff_jitter: f32,
    pub status_forcelist: HashSet<u16>,
    pub allowed_methods: HashSet<String>,
    pub respect_retry_after_header: bool,
    pub raise_on_status: bool,
    pub total_timeout: Option<f64>,
}

impl Retry {
    pub fn new(
        total: Option<i32>,
        connect: Option<i32>,
        read: Option<i32>,
        status: Option<i32>,
        backoff_factor: Option<f32>,
        backoff_max: Option<f32>,
        backoff_jitter: Option<f32>,
        status_forcelist: Option<HashSet<u16>>,
        allowed_methods: Option<HashSet<String>>,
        respect_retry_after_header: Option<bool>,
        raise_on_status: Option<bool>,
        total_timeout: Option<f64>,
    ) -> Self {
        let default_total = total.unwrap_or(DEFAULT_TOTAL_RETRIES);
        Self {
            total: default_total,
            connect: connect.unwrap_or(default_total),
            read: read.unwrap_or(default_total),
            status: status.unwrap_or(default_total),
            backoff_factor: backoff_factor.unwrap_or(DEFAULT_BACKOFF_FACTOR),
            backoff_max: backoff_max.unwrap_or(DEFAULT_BACKOFF_MAX),
            backoff_jitter: backoff_jitter.unwrap_or(DEFAULT_BACKOFF_JITTER),
            status_forcelist: status_forcelist
                .unwrap_or(DEFAULT_STATUS_FORCELIST.iter().copied().collect()),
            allowed_methods: allowed_methods.unwrap_or(
                DEFAULT_ALLOWED_METHODS
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
            respect_retry_after_header: respect_retry_after_header
                .unwrap_or(DEFAULT_RESPECT_RETRY_AFTER_HEADER),
            raise_on_status: raise_on_status.unwrap_or(DEFAULT_RAISE_ON_STATUS),
            total_timeout,
        }
    }

    pub fn allows_another(&self, kind: FailureKind, used: &RetryCounts) -> bool {
        if used.total >= self.total {
            return false;
        }
        match kind {
            FailureKind::Connect => used.connect < self.connect,
            FailureKind::Read => used.read < self.read,
            FailureKind::Status => used.status < self.status,
        }
    }

    pub fn calculate_backoff_for_attempt(&self, attempt: i32) -> f32 {
        let mut backoff = self.backoff_factor * 2_f32.powi(attempt - 1);
        // Apply jitter to spread out retries from many concurrent clients.
        if self.backoff_jitter > 0.0 {
            // Multiplier: (1 + uniform(-jitter, +jitter)) — so jitter=0.5 means
            // backoff varies ±50% from the deterministic value.
            let jitter = rand::random::<f32>() * 2.0 - 1.0; // [-1, 1)
            backoff = (backoff * (1.0 + jitter * self.backoff_jitter)).max(0.0);
        }
        return backoff;
    }
}
