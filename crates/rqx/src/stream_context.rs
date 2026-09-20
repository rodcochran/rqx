use std::sync::{Arc, Mutex};

use pyo3::prelude::{Py, PyAny, PyResult, Python, pyclass, pymethods};
use pyo3::{Bound, PyRef, PyRefMut};

use super::client::{Client, block_on_inner};
use super::exceptions::RqxError;
use super::request::RequestSpec;
use super::runtime::RUNTIME;
use super::stream::{PyAsyncStreamResponse, PyStreamResponse};

use rqx_core::stream_context::Unsent;

/// What `Client.stream()` returns: `with` sends the request and yields the
/// response, leaving the block closes it.
#[pyclass]
pub struct PyStreamContext {
    unsent: Option<Unsent>,
    response: Option<Py<PyStreamResponse>>,
}

#[pymethods]
impl PyStreamContext {
    fn __enter__(mut slf: PyRefMut<'_, Self>, py: Python<'_>) -> PyResult<Py<PyStreamResponse>> {
        let unsent = Unsent::take(&mut slf.unsent)?;
        let pending = block_on_inner(
            py,
            unsent.client.stream(
                unsent.request,
                unsent.follow_redirects,
            ),
        )?;
        let response = Py::new(
            py,
            PyStreamResponse::from_pending(pending),
        )?;
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
            unsent: Some(
                Unsent {
                    client,
                    request,
                    follow_redirects,
                },
            ),
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
    ) -> PyResult<Bound<'py, PyAny>> {
        let unsent = Unsent::take(&mut slf.unsent)?;
        let slot = Arc::clone(&slf.response);
        RUNTIME.future_into_py(
            py,
            async move {
                let pending = unsent
                    .client
                    .stream(
                        unsent.request,
                        unsent.follow_redirects,
                    )
                    .await?;
                Python::attach(
                    |py| {
                        let response = Py::new(
                            py,
                            PyAsyncStreamResponse::from_pending(pending),
                        )?;
                        *slot.lock().unwrap() = Some(response.clone_ref(py));
                        Ok(response)
                    },
                )
            },
        )
    }

    fn __aexit__<'py>(
        slf: PyRef<'py, Self>,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let slot = Arc::clone(&slf.response);
        RUNTIME.future_into_py(
            py,
            async move {
                let response = slot.lock().unwrap().take();
                if let Some(response) = response {
                    let body = Python::attach(|py| response.borrow(py).take_body());
                    if let Some(body) = body {
                        body.close().await;
                    }
                }
                Ok(false)
            },
        )
    }
}

impl PyAsyncStreamContext {
    pub fn new(client: Client, request: RequestSpec, follow_redirects: Option<bool>) -> Self {
        Self {
            unsent: Some(
                Unsent {
                    client,
                    request,
                    follow_redirects,
                },
            ),
            response: Arc::new(Mutex::new(None)),
        }
    }
}
