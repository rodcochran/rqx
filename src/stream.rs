use bytes::Bytes;
use encoding_rs::{Decoder, UTF_8};
use futures::{Stream, StreamExt};
use pyo3::Bound;
use pyo3::prelude::{Py, PyAny, PyRef, PyRefMut, PyResult, Python, pyclass, pymethods};
use reqwest::Response;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use super::exceptions::*;
use super::runtime::RUNTIME;

/// Streaming HTTP body source. `Pin<Box<dyn ...>>` is standard practice for
/// storing an erased, async-trait-object Stream: `dyn Stream` is unsized
/// (hence Box), the stream internally self-references its connection state
/// so it must not move once polled (hence Pin), and `+ Send` lets it cross
/// thread boundaries when shared via `Arc`.
type ChunkStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

#[pyclass]
struct PyByteIterator {
    stream: Arc<TokioMutex<ChunkStream>>,
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
                .expect("runtime not initialized")
                .block_on(async {
                    // .lock().await on TokioMutex — yields on contention
                    // instead of blocking the OS thread. Guard is still held
                    // across the next await, but now it's an async-aware
                    // guard, which is what clippy wants.
                    let mut guard = stream.lock().await;
                    guard.as_mut().next().await
                })
        });

        match chunk {
            Some(Ok(bytes)) => Ok(Some(bytes.to_vec())),
            Some(Err(e)) => Err(RqxError::new_err(format!("stream error: {e}"))),
            None => Ok(None),
        }
    }
}

#[pyclass]
struct PyTextIterator {
    stream: Arc<TokioMutex<ChunkStream>>,
    // Currently accepted from the Python `iter_bytes(chunk_size=...)` call but
    // not applied: reqwest's `Response::bytes_stream()` yields chunks as they
    // arrive from the network, not at caller-chosen boundaries. Matching
    // `chunk_size` semantics would require buffering here (accumulate bytes
    // until we have `chunk_size` of them, then yield). Kept on the struct so
    // the plumbing is in place when we implement that.
    #[allow(dead_code)]
    chunk_size: u32,
    decoder: Decoder,
    finished: bool,
}

#[pymethods]
impl PyTextIterator {
    fn __iter__(slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<String>> {
        if slf.finished {
            return Ok(None);
        }

        let py = slf.py();
        let stream = Arc::clone(&slf.stream);

        // need to try to decode chunks with slf.decoder
        let dst = &mut String::new();

        loop {
            if !dst.is_empty() {
                break;
            }

            let chunk = py.detach(|| {
                RUNTIME
                    .get()
                    .expect("runtime not initialized")
                    .block_on(async {
                        // .lock().await on TokioMutex — yields on contention
                        // instead of blocking the OS thread. Guard is still held
                        // across the next await, but now it's an async-aware
                        // guard, which is what clippy wants.
                        let mut guard = stream.lock().await;
                        guard.as_mut().next().await.transpose()
                    })
            });

            let c = match chunk {
                Ok(_c) => _c,
                Err(e) => {
                    return Err(RqxError::new_err(format!("stream error: {e}")));
                }
            };

            let src = match c {
                Some(_c) => _c,
                None => {
                    // No bytes - end of stream
                    slf.finished = true;
                    break;
                }
            };

            // dst gets mutated inside decoder
            let _decode_result = slf.decoder.decode_to_string(&src, dst, false);
        }

        // Flush decoder
        if slf.finished {
            let _flush_result = slf.decoder.decode_to_string(&[], dst, true);
            if dst.is_empty() {
                return Ok(None);
            }
        }

        return Ok(Some(dst.to_string()));
    }
}

/*
Async Support
*/

#[pyclass]
struct PyAsyncByteIterator {
    stream: Arc<TokioMutex<ChunkStream>>,
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
                Some(Err(e)) => Err(RqxError::new_err(format!("stream error: {e}"))),
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
        let response = self
            .response
            .take()
            .ok_or_else(|| RqxError::new_err("response already consumed"))?;
        Ok(PyByteIterator {
            stream: Arc::new(TokioMutex::new(Box::pin(response.bytes_stream()))),
            chunk_size,
        })
    }

    #[pyo3(signature = (chunk_size=8192))]
    fn iter_text(&mut self, chunk_size: u32) -> PyResult<PyTextIterator> {
        let response = self
            .response
            .take()
            .ok_or_else(|| RqxError::new_err("response already consumed"))?;

        // should add an encoding override on the struct.
        let encoding = UTF_8;
        let decoder = encoding.new_decoder();

        Ok(PyTextIterator {
            stream: Arc::new(TokioMutex::new(Box::pin(response.bytes_stream()))),
            chunk_size,
            decoder: decoder,
            finished: false,
        })
    }

    // #[pyo3(signature = (chunk_size=8192))]
    // fn iter_bytes(&mut self, chunk_size: u32) -> PyResult<PyByteIterator> {}

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

    /// `True` for 1xx responses.
    #[getter]
    fn is_informational(&self) -> bool {
        (100..200).contains(&self.status_code)
    }

    /// `True` for 2xx responses.
    #[getter]
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    /// `True` for 3xx responses that carry a `Location` header.
    ///
    /// Mirrors httpx: a 3xx without Location (e.g. 304 Not Modified) isn't
    /// classified as a redirect because nothing can follow it.
    #[getter]
    fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status_code)
            && self
                .headers
                .keys()
                .any(|k| k.eq_ignore_ascii_case("location"))
    }

    /// `True` for 4xx responses.
    #[getter]
    fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status_code)
    }

    /// `True` for 5xx responses.
    #[getter]
    fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status_code)
    }

    /// `True` for any 4xx or 5xx response.
    #[getter]
    fn is_error(&self) -> bool {
        (400..600).contains(&self.status_code)
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
        let http_version = format!("{:?}", response.version());
        let cookies: HashMap<String, String> = response
            .cookies()
            .map(|c| (c.name().to_string(), c.value().to_string()))
            .collect();

        Ok(PyStreamResponse {
            status_code: status_code,
            headers: headers,
            url: url,
            elapsed: 0.0,
            num_retries: 0,
            retry_history: Vec::new(),
            http_version: http_version,
            cookies: cookies,
            response: Some(response),
        })
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
    fn aiter_bytes(&mut self, chunk_size: u32) -> PyResult<PyAsyncByteIterator> {
        let response = self
            .response
            .take()
            .ok_or_else(|| RqxError::new_err("response already consumed"))?;
        Ok(PyAsyncByteIterator {
            stream: Arc::new(TokioMutex::new(Box::pin(response.bytes_stream()))),
            chunk_size,
        })
    }
    fn __aenter__<'py>(slf: Py<Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(slf) })
    }

    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(false) })
    }

    /// `True` for 1xx responses.
    #[getter]
    fn is_informational(&self) -> bool {
        (100..200).contains(&self.status_code)
    }

    /// `True` for 2xx responses.
    #[getter]
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    /// `True` for 3xx responses that carry a `Location` header.
    ///
    /// Mirrors httpx: a 3xx without Location (e.g. 304 Not Modified) isn't
    /// classified as a redirect because nothing can follow it.
    #[getter]
    fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status_code)
            && self
                .headers
                .keys()
                .any(|k| k.eq_ignore_ascii_case("location"))
    }

    /// `True` for 4xx responses.
    #[getter]
    fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status_code)
    }

    /// `True` for 5xx responses.
    #[getter]
    fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status_code)
    }

    /// `True` for any 4xx or 5xx response.
    #[getter]
    fn is_error(&self) -> bool {
        (400..600).contains(&self.status_code)
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
        let http_version = format!("{:?}", response.version());
        let cookies: HashMap<String, String> = response
            .cookies()
            .map(|c| (c.name().to_string(), c.value().to_string()))
            .collect();

        Ok(PyAsyncStreamResponse {
            status_code: status_code,
            headers: headers,
            url: url,
            elapsed: 0.0,
            num_retries: 0,
            retry_history: Vec::new(),
            http_version: http_version,
            cookies: cookies,
            response: Some(response),
        })
    }
}
