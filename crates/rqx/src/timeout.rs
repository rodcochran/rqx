use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;

use rqx_core::timeout::Timeout;

#[pyclass(name = "Timeout", skip_from_py_object)]
#[derive(Clone)]
pub struct PyTimeout {
    pub inner: Timeout,
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
            inner: Timeout::new(
                connect.or(all),
                read.or(all),
                write.or(all),
                pool.or(all),
            ),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Timeout(connect={:?}, read={:?}, write={:?}, pool={:?})",
            self.inner.connect, self.inner.read, self.inner.write, self.inner.pool
        )
    }

    #[getter]
    pub fn connect(&self) -> Option<f64> {
        self.inner.connect
    }

    #[getter]
    pub fn read(&self) -> Option<f64> {
        self.inner.read
    }

    #[getter]
    pub fn write(&self) -> Option<f64> {
        self.inner.write
    }

    #[getter]
    pub fn pool(&self) -> Option<f64> {
        self.inner.pool
    }
}

impl PyTimeout {
    /// Per-request total timeout (reqwest's `.timeout()` takes one Duration).
    ///
    /// Prefer `read` since it's the most common "this individual request is
    /// taking too long" phase. Fall back to the max of any other set fields.
    /// Returns None when all phases are None.
    pub fn per_request_total(&self) -> Option<f64> {
        if let Some(r) = self.inner.read {
            return Some(r);
        }
        let mut max: Option<f64> = None;
        for v in [self.inner.connect, self.inner.write, self.inner.pool] {
            if let Some(x) = v {
                max = Some(
                    max.map_or(
                        x,
                        |m| m.max(x),
                    ),
                );
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
            return Ok(
                Self {
                    inner: Timeout {
                        connect: Some(n),
                        read: Some(n),
                        write: Some(n),
                        pool: Some(n),
                    },
                },
            );
        }
        Err(PyTypeError::new_err("timeout must be a number or rqx.Timeout instance"))
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
