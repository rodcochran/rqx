use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::{Response};
use std::collections::HashMap;
use std::pin::{Pin};
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as TokioMutex};
use pyo3::Bound;
use pyo3::prelude::{Py, PyAny, PyRef, PyResult, Python, pyclass, pymethods};

use super::runtime::RUNTIME;
use super::exceptions::*;


#[pyclass]
struct PyByteIterator {
    stream: Arc<Mutex<Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>>>,
    // Currently accepted from the Python `iter_bytes(chunk_size=...)` call but
    // not applied: reqwest's `Response::bytes_stream()` yields chunks as they
    // arrive from the network, not at caller-chosen boundaries. Matching
    // `chunk_size` semantics would require buffering here (accumulate bytes
    // until we have `chunk_size` of them, then yield). Kept on the struct so
    // the plumbing is in place when we implement that.
    #[allow(dead_code)]
    chunk_size: u32,
}


#[pymethods]
impl PyByteIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(slf: PyRef<'_, Self>) -> PyResult<Option<Vec<u8>>> {

        let py = slf.py();
        let stream = Arc::clone(&slf.stream);

        let chunk = py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| ReqxError::new_err("runtime not initialized")).unwrap()
                .block_on(async {
                    let mut guard = stream.lock().unwrap();
                    guard.as_mut().next().await
                })
        });

        match chunk {
            Some(Ok(bytes)) => Ok(Some(bytes.to_vec())),
            Some(Err(e)) => Err(ReqxError::new_err(format!("stream error: {e}"))),
            None => Ok(None),
        }
    }
}

/*
Async Support
*/


#[pyclass]
struct PyAsyncByteIterator {
    stream: Arc<TokioMutex<Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>>>,
    // See PyByteIterator.chunk_size — same situation on the async path.
    #[allow(dead_code)]
    chunk_size: u32,
}


#[pymethods]
impl PyAsyncByteIterator {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__<'py>(slf: PyRef<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let stream = Arc::clone(&slf.stream);
  
        pyo3_async_runtimes::tokio::future_into_py(slf.py(), async move {
            let mut guard = stream.lock().await;
            match guard.as_mut().next().await {
                Some(Ok(bytes)) => Ok(Some(bytes.to_vec())),
                Some(Err(e)) => Err(ReqxError::new_err(format!("stream error: {e}"))),
                None => Err(pyo3::exceptions::PyStopAsyncIteration::new_err(())),
            }
        })
    }
}


/*
Response object
*/

#[pyclass]
pub struct PyStreamResponse {
    #[pyo3(get)]
    pub status_code: u16,

    #[pyo3(get)]
    pub headers: HashMap<String, String>,
    
    #[pyo3(get)]
    pub url: String,

    #[pyo3(get)]
    pub(crate) elapsed: f64,

    #[pyo3(get)]
    pub(crate) num_retries: i32,

    #[pyo3(get)]
    pub(crate) retry_history: Vec<(String, f64)>,

    #[pyo3(get)]
    pub(crate) http_version: String,

    #[pyo3(get)]
    pub(crate) cookies: HashMap<String, String>,

    pub(crate) response: Option<reqwest::Response>,
}

#[pymethods]
impl PyStreamResponse {
    /// Iterate over response bytes as they arrive.
    ///
    /// NOTE: `chunk_size` is currently accepted for API compatibility but is
    /// not enforced — chunks are yielded with whatever boundaries reqwest
    /// delivers from the network, typically 8–64 KB depending on the socket
    /// and server behavior. If you need fixed-size chunks, buffer on the
    /// caller side. Honoring `chunk_size` requires internal buffering that
    /// we haven't wired up yet.
    #[pyo3(signature = (chunk_size=8192))]
    fn iter_bytes(&mut self, chunk_size: u32) -> PyResult<PyByteIterator> {
        let response = self.response.take()
            .ok_or_else(|| ReqxError::new_err("response already consumed"))?;
        Ok(
            PyByteIterator {
                stream: Arc::new(Mutex::new(Box::pin(response.bytes_stream()))),
                chunk_size,
            }
        )
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
  
    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) {
        self.response = None; // drops the response, closes connection
    }

}

impl PyStreamResponse {

    pub fn from_response(response: Response) -> PyResult<PyStreamResponse> {
        let status_code = response.status().as_u16();

        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.to_str().unwrap_or("<non-utf8>").to_string(),
                )
            })
            .collect::<HashMap<_, _>>();
        
        let url = response.url().as_str().to_owned();
        let http_version  = format!("{:?}", response.version());
        let cookies: HashMap<String, String> = response.cookies()
            .map(|c| (
                c.name().to_string(), 
                c.value().to_string())
            )
            .collect();

        Ok(
            PyStreamResponse  {
                status_code: status_code,
                headers: headers,
                url: url,
                elapsed: 0.0,
                num_retries: 0,
                retry_history: Vec::new(),
                http_version: http_version,
                cookies: cookies,
                response: Some(response),
            }
        )

    }
}

#[pyclass]
pub struct PyAsyncStreamResponse {
    #[pyo3(get)]
    pub status_code: u16,

    #[pyo3(get)]
    pub headers: HashMap<String, String>,
    
    #[pyo3(get)]
    pub url: String,

    #[pyo3(get)]
    pub(crate) elapsed: f64,

    #[pyo3(get)]
    pub(crate) num_retries: i32,

    #[pyo3(get)]
    pub(crate) retry_history: Vec<(String, f64)>,

    #[pyo3(get)]
    pub(crate) http_version: String,

    #[pyo3(get)]
    pub(crate) cookies: HashMap<String, String>,

    pub(crate) response: Option<reqwest::Response>,
}


#[pymethods]
impl PyAsyncStreamResponse {
    /// Async iterate over response bytes as they arrive.
    ///
    /// NOTE: `chunk_size` is currently accepted for API compatibility but is
    /// not enforced — chunks are yielded with whatever boundaries reqwest
    /// delivers from the network. See PyStreamResponse::iter_bytes for the
    /// full explanation.
    #[pyo3(signature = (chunk_size=8192))]
    fn iter_bytes(&mut self, chunk_size: u32) -> PyResult<PyAsyncByteIterator> {
        let response = self.response.take()
            .ok_or_else(|| ReqxError::new_err("response already consumed"))?;
        Ok(
            PyAsyncByteIterator {
                stream: Arc::new(TokioMutex::new(Box::pin(response.bytes_stream()))),
                chunk_size,
            }
        )
    }
    fn __aenter__<'py>(slf: Py<Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(slf)
        })
    }

    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(false)
        })
    }
}

impl PyAsyncStreamResponse {

    pub fn from_response(response: Response) -> PyResult<PyAsyncStreamResponse> {
        let status_code = response.status().as_u16();

        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.to_str().unwrap_or("<non-utf8>").to_string(),
                )
            })
            .collect::<HashMap<_, _>>();
        
        let url = response.url().as_str().to_owned();
        let http_version  = format!("{:?}", response.version());
        let cookies: HashMap<String, String> = response.cookies()
            .map(|c| (
                c.name().to_string(), 
                c.value().to_string())
            )
            .collect();

        Ok(
            PyAsyncStreamResponse  {
                status_code: status_code,
                headers: headers,
                url: url,
                elapsed: 0.0,
                num_retries: 0,
                retry_history: Vec::new(),
                http_version: http_version,
                cookies: cookies,
                response: Some(response),
            }
        )

    }
}
