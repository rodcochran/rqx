use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use encoding_rs::{Decoder, UTF_8};
use futures::{Stream, StreamExt};
use pyo3::Bound;
use pyo3::prelude::{Py, PyAny, PyRef, PyRefMut, PyResult, Python, pyclass, pymethods};
use pyo3::sync::PyOnceLock;
use pyo3::types::PyBytes;
use reqwest::Response;
use tokio::sync::Mutex as TokioMutex;

use super::exceptions::*;
use super::headers::PyHeaders;
use super::py_json::value_to_py;
use super::response::ResponseParts;
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

pub enum Body {
    Live(reqwest::Response),
    Buffered(Bytes),
}

#[pyclass]
pub struct PyStreamResponse {
    pub parts: ResponseParts,
    pub(crate) body: Option<Body>,
    pub content_cache: PyOnceLock<Py<PyBytes>>,
}

#[pymethods]
impl PyStreamResponse {
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) {
        self.body = None; // drops the response, closes connection
    }

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
        let response = match self.body.take() {
            Some(Body::Live(r)) => r,
            Some(other) => {
                // Buffered — restore it, then error
                self.body = Some(other);
                return Err(RqxError::new_err("response already read into memory"));
            }
            None => return Err(RqxError::new_err("response already consumed or closed")),
        };
        return Ok(PyByteIterator {
            stream: Arc::new(TokioMutex::new(Box::pin(response.bytes_stream()))),
            chunk_size,
        });
    }

    #[pyo3(signature = (chunk_size=8192))]
    fn iter_text(&mut self, chunk_size: u32) -> PyResult<PyTextIterator> {
        let response = match self.body.take() {
            Some(Body::Live(r)) => r,
            Some(other) => {
                // Buffered — restore it, then error
                self.body = Some(other);
                return Err(RqxError::new_err("response already read into memory"));
            }
            None => return Err(RqxError::new_err("response already consumed or closed")),
        };

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

    fn read(&mut self, py: Python<'_>) -> PyResult<Py<PyBytes>> {
        match self.body.take() {
            Some(Body::Live(response)) => {
                let bytes = py
                    .detach(|| {
                        RUNTIME
                            .get()
                            .expect("runtime not initialized")
                            .block_on(async { response.bytes().await })
                    })
                    .map_err(map_reqwest_error)?;
                self.body = Some(Body::Buffered(bytes));
            }
            Some(buffered) => self.body = Some(buffered), // already Buffered — restore unchanged
            None => return Err(RqxError::new_err("response already consumed or closed")),
        }
        self.content(py) // single, cached materialization — shared with the .content getter
    }

    #[getter]
    fn content(&self, py: Python<'_>) -> PyResult<Py<PyBytes>> {
        match &self.body {
            Some(Body::Buffered(bytes)) => Ok(self
                .content_cache
                .get_or_init(py, || PyBytes::new(py, bytes).unbind())
                .clone_ref(py)),
            Some(Body::Live(_)) => Err(RqxError::new_err("response not read; call read() first")),
            None => Err(RqxError::new_err("response consumed or closed")),
        }
    }

    #[getter]
    fn text(&self) -> PyResult<String> {
        match &self.body {
            Some(Body::Buffered(bytes)) => {
                let encoding = self.parts.resolved_encoding();
                let (decoded, _, _) = encoding.decode(bytes);
                Ok(decoded.into_owned())
            }
            Some(Body::Live(_)) => Err(RqxError::new_err("response not read; call read() first")),
            None => Err(RqxError::new_err("response consumed or closed")),
        }
    }

    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.body {
            Some(Body::Buffered(bytes)) => {
                let value = match serde_json::from_slice(bytes) {
                    Ok(v) => v,
                    Err(e) => {
                        let content_type = self.parts.content_type().unwrap_or("<none>");
                        let preview_len = bytes.len().min(100);
                        let preview = String::from_utf8_lossy(&bytes[..preview_len]);
                        let ellipsis = if bytes.len() > 100 { "..." } else { "" };
                        return Err(RqxError::new_err(format!(
                            "response is not JSON (HTTP {}, content-type: {}): {:?}{} ({})",
                            self.parts.status_code, content_type, preview, ellipsis, e
                        )));
                    }
                };

                value_to_py(py, value)
            }
            Some(Body::Live(_)) => Err(RqxError::new_err("response not read; call read() first")),
            None => Err(RqxError::new_err("response consumed or closed")),
        }
    }

    #[getter]
    fn status_code(&self) -> u16 {
        self.parts.status_code
    }

    #[getter]
    fn headers(&self) -> PyHeaders {
        // can we figure out a way to have this referenced instead of cloned?
        PyHeaders::from_header_map(self.parts.headers.clone())
    }

    #[getter]
    fn url(&self) -> &str {
        &self.parts.url
    }

    #[getter]
    fn elapsed(&self) -> f64 {
        self.parts.elapsed
    }

    #[getter]
    fn num_retries(&self) -> u32 {
        self.parts.num_retries
    }

    #[getter]
    fn retry_history(&self) -> &[(String, f64)] {
        &self.parts.retry_history
    }

    #[getter]
    fn http_version(&self) -> &str {
        &self.parts.http_version
    }

    #[getter]
    fn cookies(&self) -> &HashMap<String, String> {
        &self.parts.cookies
    }

    #[getter]
    fn encoding_override(&self) -> &Option<String> {
        // potential to have return value &Option<str>
        &self.parts.encoding_override
    }

    #[getter]
    fn encoding(&self) -> String {
        self.parts.encoding()
    }

    /// Override the encoding used by `.text`. Set to any encoding label
    /// `encoding_rs` understands ("utf-8", "iso-8859-1", "windows-1252", ...).
    /// Invalid labels silently fall back to UTF-8 when decoding.
    #[setter]
    fn set_encoding(&mut self, value: String) {
        self.parts.encoding_override = Some(value);
    }

    #[getter]
    fn is_informational(&self) -> bool {
        self.parts.is_informational()
    }

    #[getter]
    fn is_success(&self) -> bool {
        self.parts.is_success()
    }

    #[getter]
    fn is_redirect(&self) -> bool {
        self.parts.is_redirect()
    }

    #[getter]
    fn is_client_error(&self) -> bool {
        self.parts.is_client_error()
    }
    #[getter]
    fn is_server_error(&self) -> bool {
        self.parts.is_server_error()
    }
    #[getter]
    fn is_error(&self) -> bool {
        self.parts.is_error()
    }
}

impl PyStreamResponse {
    pub fn from_response(response: Response) -> PyResult<PyStreamResponse> {
        Ok(PyStreamResponse {
            parts: ResponseParts::from_reqwest(&response),
            body: Some(Body::Live(response)),
            content_cache: PyOnceLock::new(),
        })
    }
}

#[pyclass]
pub struct PyAsyncStreamResponse {
    pub parts: ResponseParts,
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

    #[getter]
    fn status_code(&self) -> u16 {
        self.parts.status_code
    }

    #[getter]
    fn headers(&self) -> PyHeaders {
        // can we figure out a way to have this referenced instead of cloned?
        PyHeaders::from_header_map(self.parts.headers.clone())
    }

    #[getter]
    fn url(&self) -> &str {
        &self.parts.url
    }

    #[getter]
    fn elapsed(&self) -> f64 {
        self.parts.elapsed
    }

    #[getter]
    fn num_retries(&self) -> u32 {
        self.parts.num_retries
    }

    #[getter]
    fn retry_history(&self) -> &[(String, f64)] {
        &self.parts.retry_history
    }

    #[getter]
    fn http_version(&self) -> &str {
        &self.parts.http_version
    }

    #[getter]
    fn cookies(&self) -> &HashMap<String, String> {
        &self.parts.cookies
    }

    #[getter]
    fn encoding_override(&self) -> &Option<String> {
        // potential to have return value &Option<str>
        &self.parts.encoding_override
    }

    #[getter]
    fn encoding(&self) -> String {
        self.parts.encoding()
    }

    /// Override the encoding used by `.text`. Set to any encoding label
    /// `encoding_rs` understands ("utf-8", "iso-8859-1", "windows-1252", ...).
    /// Invalid labels silently fall back to UTF-8 when decoding.
    #[setter]
    fn set_encoding(&mut self, value: String) {
        self.parts.encoding_override = Some(value);
    }

    #[getter]
    fn is_informational(&self) -> bool {
        self.parts.is_informational()
    }

    #[getter]
    fn is_success(&self) -> bool {
        self.parts.is_success()
    }

    #[getter]
    fn is_redirect(&self) -> bool {
        self.parts.is_redirect()
    }

    #[getter]
    fn is_client_error(&self) -> bool {
        self.parts.is_client_error()
    }
    #[getter]
    fn is_server_error(&self) -> bool {
        self.parts.is_server_error()
    }
    #[getter]
    fn is_error(&self) -> bool {
        self.parts.is_error()
    }
}

impl PyAsyncStreamResponse {
    pub fn from_response(response: Response) -> PyResult<PyAsyncStreamResponse> {
        Ok(PyAsyncStreamResponse {
            parts: ResponseParts::from_reqwest(&response),
            response: Some(response),
        })
    }
}
