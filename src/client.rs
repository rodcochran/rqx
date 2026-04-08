use pyo3::PyTypeInfo;
use pyo3::ffi::newfunc;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PySuper, PyTuple};

use core::panic;
use std::iter::Map;
use reqwest::Client;
use std::time::Duration;
use tokio::runtime::Runtime;

use super::runtime::RUNTIME;

#[derive(FromPyObject)]
pub struct Config {
    timeout: Duration,
}

pub struct PyClient {
    http_client: Client,
    config: Config,
}

//PyResponse with .status_code, .headers, .text(), .content, .json()

pub struct PyResponse {
    status_code: i32,
    headers: Map<String, String>,
    content: String,
}

impl PyResponse {

    pub fn text(&self) {
        
    }
}




#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (config))]
    fn __new__(config: Config) -> PyResult<Self> {
        let http_client = Client::builder()
            .timeout(config.timeout)
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .brotli(true)
            .pool_max_idle_per_host(20)
            .build()
            .expect("Failed to build HTTP client");
        Ok(Self {
            http_client,
            config,
        })
    }

    #[pyo3(signature = (url))]
    pub fn get(&self, url: &str) -> PyResult<> {
        let request = match self
            .http_client
            .get(url)
            .timeout(self.config.timeout)
            .build()
        {
            Ok(req) => req,
            Err(e) => return false,
        };

        let response = RUNTIME.get().unwrap().block_on(async {
            match self.http_client.execute(request).await {
                Ok(req) => req,
                Err(e) => panic!("{}", e),
            }
        });

        response.status()
        true
    }
}
