use std::sync::{Arc, Mutex};

use pyo3::prelude::{Py, PyAny, PyResult, Python, pyclass, pymethods};
use pyo3::{Bound, PyRef, PyRefMut};

use rqx_core::client::Client;
use rqx_core::request::RequestSpec;
use rqx_core::streaming::context::Unsent;

use super::client::block_on_inner;
use super::exceptions::PyRqxError;
use super::runtime::RUNTIME;
use super::stream::{PyAsyncStreamResponse, PyStreamResponse};

/// What `Client.stream()` returns: `with` sends the request and yields the
/// response, leaving the block closes it.
#[pyclass]
pub struct PyStreamContext {
    unsent: Option<Unsent>,
    response: Option<Py<PyStreamResponse>>,
}

#[pymethods]
impl PyStreamContext {
    fn __enter__(
        mut slf: PyRefMut<'_, Self>,
        py: Python<'_>,
    ) -> Result<Py<PyStreamResponse>, PyRqxError> {
        let unsent = Unsent::take(&mut slf.unsent)?;
        let pending = block_on_inner(py, unsent.send())?;
        let response = Py::new(py, PyStreamResponse::from_pending(pending))?;
        slf.response = Some(response.clone_ref(py));
        Ok(response)
    }

    fn __exit__(
        mut slf: PyRefMut<'_, Self>,
        py: Python<'_>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) {
        if let Some(response) = slf.response.take() {
            response.borrow_mut(py).close();
        }
    }
}

impl PyStreamContext {
    pub fn new(client: Client, request: RequestSpec, follow_redirects: Option<bool>) -> Self {
        Self {
            unsent: Some(Unsent::new(client, request, follow_redirects)),
            response: None,
        }
    }
}

/// What `AsyncClient.stream()` returns: `async with` sends the request and
/// yields the response, leaving the block closes it.
#[pyclass]
pub struct PyAsyncStreamContext {
    unsent: Option<Unsent>,
    // Filled in by the enter future, which cannot hold `&mut self` across its await.
    response: Arc<Mutex<Option<Py<PyAsyncStreamResponse>>>>,
}

#[pymethods]
impl PyAsyncStreamContext {
    fn __aenter__<'py>(
        mut slf: PyRefMut<'py, Self>,
        py: Python<'py>,
    ) -> Result<Bound<'py, PyAny>, PyRqxError> {
        let unsent = Unsent::take(&mut slf.unsent)?;
        let slot = Arc::clone(&slf.response);
        Ok(RUNTIME.future_into_py(py, async move {
            let pending = unsent.send().await?;
            Python::attach(|py| {
                let response = Py::new(py, PyAsyncStreamResponse::from_pending(pending))?;
                *slot.lock().unwrap() = Some(response.clone_ref(py));
                Ok::<_, PyRqxError>(response)
            })
        })?)
    }

    fn __aexit__<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let slot = Arc::clone(&slf.response);
        RUNTIME.future_into_py(py, async move {
            let response = slot.lock().unwrap().take();
            if let Some(response) = response {
                let body = Python::attach(|py| response.borrow(py).take_body());
                if let Some(body) = body {
                    body.close().await;
                }
            }
            Ok::<_, PyRqxError>(false)
        })
    }
}

impl PyAsyncStreamContext {
    pub fn new(client: Client, request: RequestSpec, follow_redirects: Option<bool>) -> Self {
        Self {
            unsent: Some(Unsent::new(client, request, follow_redirects)),
            response: Arc::new(Mutex::new(None)),
        }
    }
}
