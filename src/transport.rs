
use reqwest::{Client, Request};
use pyo3::prelude::{Py, PyAny, PyRef, PyResult, Python,  pyclass, pymethods};

use super::retry::PyRetry;

#[pyclass]
pub struct HttpTransport {
    http_client: Client,
    retries: PyRetry
}

#[pymethods]
impl HttpTransport {
    #[new]
    #[pyo3(signature = (retries=None))]
    fn __new__(
        retries: Option<PyRef<'_, PyRetry>>,
    ) -> PyResult<Self> {

        let _retries = match retries {
            Some(r) => r.clone(),
            None => PyRetry::with_defaults(),
        };

        let http_client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(20)
            .build()
            .expect("Failed to build HTTP client");
        Ok(Self {
            http_client: http_client,
            retries: _retries
        })
    }
}