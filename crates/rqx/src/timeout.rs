use pyo3::prelude::*;

use rqx_core::timeout::Timeout;

use crate::py_utils::repr::PyRepr;

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
            inner: Timeout::new(connect.or(all), read.or(all), write.or(all), pool.or(all)),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Timeout(connect={}, read={}, write={}, pool={})",
            self.inner.connect.py_repr(),
            self.inner.read.py_repr(),
            self.inner.write.py_repr(),
            self.inner.pool.py_repr(),
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

/// The `timeout=` argument: a bare number (every phase) or an `rqx.Timeout`.
#[derive(FromPyObject)]
pub enum TimeoutArg<'py> {
    #[pyo3(annotation = "Timeout")]
    Timeout(PyRef<'py, PyTimeout>),
    #[pyo3(annotation = "float")]
    Seconds(f64),
}

impl From<TimeoutArg<'_>> for Timeout {
    fn from(arg: TimeoutArg<'_>) -> Self {
        match arg {
            TimeoutArg::Timeout(t) => t.inner.clone(),
            TimeoutArg::Seconds(n) => Timeout::new(Some(n), Some(n), Some(n), Some(n)),
        }
    }
}
