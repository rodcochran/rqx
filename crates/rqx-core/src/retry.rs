use pyo3::prelude::{PyResult, pyclass, pymethods};
use std::collections::HashSet;

const DEFAULT_TOTAL_RETRIES: i32 = 3;
const DEFAULT_BACKOFF_FACTOR: f32 = 0.0;
const DEFAULT_BACKOFF_MAX: f32 = 120.0;
const DEFAULT_BACKOFF_JITTER: f32 = 0.0;
const DEFAULT_STATUS_FORCELIST: &[u16] = &[];
const DEFAULT_ALLOWED_METHODS: &[&str] = &[
    "DELETE", "GET", "HEAD", "OPTIONS", "PUT",
    // "TRACE"
];
const DEFAULT_RESPECT_RETRY_AFTER_HEADER: bool = true;
const DEFAULT_RAISE_ON_STATUS: bool = true;
pub(crate) const DEFAULT_RAISE_ON_REDIRECT: bool = true;
const DEFAULT_TOTAL_TIMEOUT: Option<f64> = None;

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
    pub raise_on_redirect: bool,
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
        raise_on_redirect: Option<bool>,
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
            raise_on_redirect: raise_on_redirect.unwrap_or(DEFAULT_RAISE_ON_REDIRECT),
            total_timeout: total_timeout,
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
}

/// `total` counts retries, not attempts: total=3 allows four attempts. Under
/// follow_redirects the caps apply per hop; num_retries and retry_history on
/// the final response add up across the chain.
#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct PyRetry {
    inner: Retry,
}

#[pymethods]
impl PyRetry {
    #[new]
    #[pyo3(signature = (
        total=None,
        connect=None,
        read=None,
        status=None,
        backoff_factor=None,
        backoff_max=None,
        backoff_jitter=None,
        status_forcelist=None,
        allowed_methods=None,
        respect_retry_after_header=None,
        raise_on_status=None,
        raise_on_redirect=None,
        total_timeout=None,
    ))]
    fn __new__(
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
        raise_on_redirect: Option<bool>,
        total_timeout: Option<f64>,
    ) -> PyResult<Self> {
        Ok(
            Self {
                inner: Retry::new(
                    total,
                    connect,
                    read,
                    status,
                    backoff_factor,
                    backoff_max,
                    backoff_jitter,
                    status_forcelist,
                    allowed_methods,
                    respect_retry_after_header,
                    raise_on_status,
                    raise_on_redirect,
                    total_timeout,
                ),
            },
        )
    }

    // maximum total retry attempts (across all failure modes)
    #[getter]
    pub fn total(&self) -> i32 {
        self.inner.total
    }

    // max retries on connection errors (defaults to total)
    #[getter]
    pub fn connect(&self) -> i32 {
        self.inner.connect
    }

    // max retries on read errors (defaults to total)
    #[getter]
    pub fn read(&self) -> i32 {
        self.inner.read
    }

    // max retries on bad status codes (defaults to total)
    #[getter]
    pub fn status(&self) -> i32 {
        self.inner.status
    }

    // multiplier for exponential backoff between retries
    #[getter]
    pub fn backoff_factor(&self) -> f32 {
        self.inner.backoff_factor
    }

    // ceiling on computed backoff delay in seconds
    #[getter]
    pub fn backoff_max(&self) -> f32 {
        self.inner.backoff_max
    }

    // random jitter added to backoff (0.0 = no jitter)
    #[getter]
    pub fn backoff_jitter(&self) -> f32 {
        self.inner.backoff_jitter
    }

    // set of status codes that trigger a retry
    #[getter]
    pub fn status_forcelist(&self) -> HashSet<u16> {
        self.inner.status_forcelist.clone()
    }

    // only retry requests with these HTTP methods
    #[getter]
    pub fn allowed_methods(&self) -> HashSet<String> {
        self.inner.allowed_methods.clone()
    }

    // honor Retry-After header delay when present
    #[getter]
    pub fn respect_retry_after_header(&self) -> bool {
        self.inner.respect_retry_after_header
    }

    // raise MaxRetriesExceeded when retries exhausted
    #[getter]
    pub fn raise_on_status(&self) -> bool {
        self.inner.raise_on_status
    }

    // raise TooManyRedirects when redirect loop detected
    #[getter]
    pub fn raise_on_redirect(&self) -> bool {
        self.inner.raise_on_redirect
    }

    // raise MaxRetriesExceeded when total time in retry exceeds max
    #[getter]
    pub fn total_timeout(&self) -> Option<f64> {
        self.inner.total_timeout
    }
}

// TODO: determine if these tests are even worth it (leaning toward no)...
//
// #[cfg(test)]
// mod tests {
//     use super::*;

//     const KINDS: [FailureKind; 3] = [FailureKind::Connect, FailureKind::Read, FailureKind::Status];

//     fn config(total: i32, connect: i32, read: i32, status: i32) -> PyRetry {
//         PyRetry {
//             inner: Retry::new(
//                 Some(total),
//                 Some(connect),
//                 Some(read),
//                 Some(status),
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//                 None,
//             ),
//         }
//     }

//     fn cap(r: &PyRetry, kind: FailureKind) -> i32 {
//         match kind {
//             FailureKind::Connect => r.inner.connect,
//             FailureKind::Read => r.inner.read,
//             FailureKind::Status => r.inner.status,
//         }
//     }

//     fn count(used: &RetryCounts, kind: FailureKind) -> i32 {
//         match kind {
//             FailureKind::Connect => used.connect,
//             FailureKind::Read => used.read,
//             FailureKind::Status => used.status,
//         }
//     }

//     /// Every config with caps in 0..=3 and every count vector in the same
//     /// range: the space is small enough to enumerate outright, which beats
//     /// sampling it (https://github.com/rodcochran/rqx/issues/44).
//     fn every_config_and_state(mut check: impl FnMut(&PyRetry, &RetryCounts)) {
//         for total in 0..=3 {
//             for connect in 0..=3 {
//                 for read in 0..=3 {
//                     for status in 0..=3 {
//                         let r = config(
//                             total, connect, read, status,
//                         );
//                         for c in 0..=3 {
//                             for rd in 0..=3 {
//                                 for st in 0..=3 {
//                                     let used = RetryCounts {
//                                         total: c + rd + st,
//                                         connect: c,
//                                         read: rd,
//                                         status: st,
//                                     };
//                                     check(
//                                         &r, &used,
//                                     );
//                                 }
//                             }
//                         }
//                     }
//                 }
//             }
//         }
//     }

//     #[test]
//     fn never_allows_past_the_total_or_the_kind_cap() {
//         every_config_and_state(
//             |r, used| {
//                 for kind in KINDS {
//                     let allowed = r.allows_another(
//                         kind, used,
//                     );
//                     if used.total >= r.inner.total
//                         || count(
//                             used, kind,
//                         ) >= cap(
//                             r, kind,
//                         )
//                     {
//                         assert!(
//                             !allowed,
//                             "allowed past a cap: {:?}",
//                             kind
//                         );
//                     } else {
//                         assert!(
//                             allowed,
//                             "refused inside every cap: {:?}",
//                             kind
//                         );
//                     }
//                 }
//             },
//         );
//     }

//     #[test]
//     fn once_refused_a_kind_stays_refused_as_counts_grow() {
//         every_config_and_state(
//             |r, used| {
//                 for kind in KINDS {
//                     if r.allows_another(
//                         kind, used,
//                     ) {
//                         continue;
//                     }
//                     for more in KINDS {
//                         let mut grown = RetryCounts {
//                             total: used.total,
//                             connect: used.connect,
//                             read: used.read,
//                             status: used.status,
//                         };
//                         grown.record(more);
//                         assert!(
//                             !r.allows_another(
//                                 kind, &grown
//                             )
//                         );
//                     }
//                 }
//             },
//         );
//     }

//     #[test]
//     fn a_single_kind_sequence_stops_at_the_smaller_cap() {
//         for total in 0..=3 {
//             for cap_value in 0..=3 {
//                 for kind in KINDS {
//                     let r = match kind {
//                         FailureKind::Connect => config(
//                             total, cap_value, 3, 3,
//                         ),
//                         FailureKind::Read => config(
//                             total, 3, cap_value, 3,
//                         ),
//                         FailureKind::Status => config(
//                             total, 3, 3, cap_value,
//                         ),
//                     };
//                     let mut used = RetryCounts::default();
//                     while r.allows_another(
//                         kind, &used,
//                     ) {
//                         used.record(kind);
//                     }
//                     assert_eq!(
//                         count(
//                             &used, kind
//                         ),
//                         total.min(cap_value)
//                     );
//                     assert_eq!(
//                         used.total,
//                         total.min(cap_value)
//                     );
//                 }
//             }
//         }
//     }
// }
