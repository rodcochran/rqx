use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;

/// Granular per-phase HTTP timeout config.
///
/// Each phase is independent and may be `None` (no timeout on that phase).
/// Phases:
///   - `connect` — TCP/TLS connection establishment
///   - `read`    — receiving response data
///   - `write`   — sending request body (currently a no-op; reqwest doesn't
///                 expose a per-phase write timeout)
///   - `pool`    — connection pool idle timeout (maps to reqwest's
///                 `pool_idle_timeout`; semantics differ slightly from httpx's
///                 pool-acquisition timeout)
///
/// Construct with a single `all` value to set every phase, or pass per-phase
/// kwargs. Per-phase kwargs take precedence over `all` when both are given.
#[pyclass(name = "Timeout", skip_from_py_object)]
#[derive(Clone)]
pub struct PyTimeout {
    #[pyo3(get)]
    pub connect: Option<f64>,
    #[pyo3(get)]
    pub read: Option<f64>,
    #[pyo3(get)]
    pub write: Option<f64>,
    #[pyo3(get)]
    pub pool: Option<f64>,
}

#[pymethods]
impl PyTimeout {
    #[new]
    #[pyo3(signature = (all=None, *, connect=None, read=None, write=None, pool=None))]
    fn __new__(
        all: Option<f64>,
        connect: Option<f64>,
        read: Option<f64>,
        write: Option<f64>,
        pool: Option<f64>,
    ) -> Self {
        Self {
            connect: connect.or(all),
            read: read.or(all),
            write: write.or(all),
            pool: pool.or(all),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Timeout(connect={:?}, read={:?}, write={:?}, pool={:?})",
            self.connect, self.read, self.write, self.pool
        )
    }
}

impl PyTimeout {
    /// Per-request total timeout (reqwest's `.timeout()` takes one Duration).
    ///
    /// Prefer `read` since it's the most common "this individual request is
    /// taking too long" phase. Fall back to the max of any other set fields.
    /// Returns None when all phases are None.
    pub fn per_request_total(&self) -> Option<f64> {
        if let Some(r) = self.read {
            return Some(r);
        }
        let mut max: Option<f64> = None;
        for v in [self.connect, self.write, self.pool] {
            if let Some(x) = v {
                max = Some(max.map_or(x, |m| m.max(x)));
            }
        }
        max
    }

    /// Extract a PyTimeout from a Python value: int, float, or PyTimeout.
    /// Plain numbers fill all four phases (matches httpx's `Timeout(n)` shortcut).
    pub fn extract_any(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(t) = value.cast::<PyTimeout>() {
            return Ok(t.borrow().clone());
        }
        if let Ok(n) = value.extract::<f64>() {
            return Ok(Self {
                connect: Some(n),
                read: Some(n),
                write: Some(n),
                pool: Some(n),
            });
        }
        Err(PyTypeError::new_err(
            "timeout must be a number or rqx.Timeout instance",
        ))
    }

    /// Resolve a per-request `timeout=` kwarg to a seconds value for
    /// `reqwest::RequestBuilder::timeout`. Accepts int, float, or rqx.Timeout
    /// (uses `read` field or max non-None as the per-request total). Falls
    /// back to `default` when nothing is passed.
    pub fn resolve_request_timeout(
        value: Option<&Bound<'_, PyAny>>,
        default: f64,
    ) -> PyResult<f64> {
        match value {
            None => Ok(default),
            Some(t) => {
                let parsed = Self::extract_any(t)?;
                Ok(parsed.per_request_total().unwrap_or(default))
            }
        }
    }
}
