use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use encoding_rs::{Decoder, Encoding};
use futures::{Stream, StreamExt};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::{
    Py, PyAny, PyAnyMethods, PyRef, PyRefMut, PyResult, Python, pyclass, pymethods,
};
use pyo3::sync::PyOnceLock;
use pyo3::types::PyBytes;
use pyo3::{Bound, IntoPyObject, PyErr};
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::Notify;

use super::client::block_on_inner;
use super::exceptions::*;
use super::headers::PyHeaders;
use super::py_json::value_to_py;
use super::response::{PendingResponse, ResponseParts};
use super::runtime::RUNTIME;
use super::url::PyURL;

use rqx_core::stream::*;

#[pyclass]
struct PyByteIterator {
    stream: LiveStream,
    chunker: ByteChunker,
    finished: bool,
}

#[pymethods]
impl PyByteIterator {
    fn __iter__(slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<Py<PyBytes>>> {
        let py = slf.py();
        let stream = slf.stream.clone();
        loop {
            if !slf.finished {
                stream.check_open()?;
            }
            if let Some(piece) = slf.chunker.next_full() {
                return Ok(
                    Some(
                        PyBytes::new(
                            py, &piece,
                        )
                        .unbind(),
                    ),
                );
            }
            if slf.finished {
                return Ok(
                    slf.chunker.flush().map(
                        |piece| {
                            PyBytes::new(
                                py, &piece,
                            )
                            .unbind()
                        },
                    ),
                );
            }
            match block_on_inner(
                py,
                stream.next_chunk(),
            )? {
                Some(bytes) => slf.chunker.feed(bytes),
                None => slf.finished = true,
            }
        }
    }
}

#[pyclass]
struct PyTextIterator {
    stream: LiveStream,
    decoder: TextDecoder,
    chunker: TextChunker,
    finished: bool,
}

#[pymethods]
impl PyTextIterator {
    fn __iter__(slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<String>> {
        let py = slf.py();
        let stream = slf.stream.clone();
        loop {
            if !slf.finished {
                stream.check_open()?;
            }
            if let Some(piece) = slf.chunker.next_full() {
                return Ok(Some(piece));
            }
            if slf.finished {
                return Ok(slf.chunker.flush());
            }
            match block_on_inner(
                py,
                stream.next_chunk(),
            )? {
                Some(src) => {
                    let text = slf.decoder.decode(
                        &src, false,
                    );
                    slf.chunker.feed(&text);
                }
                // End of stream: flush any character the decoder still holds.
                None => {
                    slf.finished = true;
                    let text = slf.decoder.decode(
                        &[],
                        true,
                    );
                    slf.chunker.feed(&text);
                }
            }
        }
    }
}

#[pyclass]
struct PyLineIterator {
    stream: LiveStream,
    decoder: TextDecoder,
    lines: LineDecoder,
    // Complete lines decoded from a chunk but not yet yielded — one chunk can
    // produce many lines, but __next__ hands back one at a time.
    pending: VecDeque<String>,
    finished: bool,
}

#[pymethods]
impl PyLineIterator {
    fn __iter__(slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<String>> {
        let py = slf.py();
        let stream = slf.stream.clone();

        loop {
            if !slf.finished {
                stream.check_open()?;
            }
            // Drain already-decoded lines before touching the network.
            if let Some(line) = slf.pending.pop_front() {
                return Ok(Some(line));
            }
            if slf.finished {
                return Ok(None);
            }

            let chunk = block_on_inner(
                py,
                stream.next_chunk(),
            )?;

            match chunk {
                Some(src) => {
                    let text = slf.decoder.decode(
                        &src, false,
                    );
                    let lines = slf.lines.feed(&text);
                    slf.pending.extend(lines);
                }
                None => {
                    // End of stream: flush the byte decoder, feed any final
                    // text through the line splitter, THEN flush the line
                    // buffer. Both flushes are required, in this order.
                    let text = slf.decoder.decode(
                        &[],
                        true,
                    );
                    let lines = slf.lines.feed(&text);
                    slf.pending.extend(lines);
                    if let Some(last) = slf.lines.flush() {
                        slf.pending.push_back(last);
                    }
                    slf.finished = true;
                }
            }
        }
    }
}

/*
Async Support
*/

struct PyBytesChunk(Bytes);

impl<'py> IntoPyObject<'py> for PyBytesChunk {
    type Target = PyBytes;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(
            PyBytes::new(
                py, &self.0,
            ),
        )
    }
}

struct AsyncByteState {
    stream: LiveStream,
    chunker: ByteChunker,
    finished: bool,
}

#[pyclass]
struct PyAsyncByteIterator {
    state: Arc<TokioMutex<AsyncByteState>>,
}

#[pymethods]
impl PyAsyncByteIterator {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__<'py>(slf: PyRef<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let state = Arc::clone(&slf.state);
        RUNTIME.future_into_py(
            slf.py(),
            async move {
                let mut s = state.lock().await;
                loop {
                    if !s.finished {
                        s.stream.check_open()?;
                    }
                    if let Some(piece) = s.chunker.next_full() {
                        return Ok(Some(PyBytesChunk(piece)));
                    }
                    if s.finished {
                        return match s.chunker.flush() {
                            Some(piece) => Ok(Some(PyBytesChunk(piece))),
                            None => Err(pyo3::exceptions::PyStopAsyncIteration::new_err(())),
                        };
                    }
                    match s.stream.next_chunk().await? {
                        Some(bytes) => s.chunker.feed(bytes),
                        None => s.finished = true,
                    }
                }
            },
        )
    }
}

struct AsyncTextState {
    stream: LiveStream,
    decoder: TextDecoder,
    chunker: TextChunker,
    finished: bool,
}

#[pyclass]
struct PyAsyncTextIterator {
    state: Arc<TokioMutex<AsyncTextState>>,
}

#[pymethods]
impl PyAsyncTextIterator {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__<'py>(slf: PyRef<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let state = Arc::clone(&slf.state);
        RUNTIME.future_into_py(
            slf.py(),
            async move {
                // Held across the chunk-pull await, so a tokio mutex (not std).
                let mut s = state.lock().await;
                loop {
                    if !s.finished {
                        s.stream.check_open()?;
                    }
                    if let Some(piece) = s.chunker.next_full() {
                        return Ok(piece);
                    }
                    if s.finished {
                        return match s.chunker.flush() {
                            Some(piece) => Ok(piece),
                            None => Err(pyo3::exceptions::PyStopAsyncIteration::new_err(())),
                        };
                    }
                    let chunk = s.stream.next_chunk().await?;
                    match chunk {
                        Some(src) => {
                            let text = s.decoder.decode(
                                &src, false,
                            );
                            s.chunker.feed(&text);
                        }
                        None => {
                            s.finished = true;
                            let text = s.decoder.decode(
                                &[],
                                true,
                            );
                            s.chunker.feed(&text);
                        }
                    }
                }
            },
        )
    }
}

struct AsyncLineState {
    stream: LiveStream,
    decoder: TextDecoder,
    lines: LineDecoder,
    pending: VecDeque<String>,
    finished: bool,
}

#[pyclass]
struct PyAsyncLineIterator {
    state: Arc<TokioMutex<AsyncLineState>>,
}

#[pymethods]
impl PyAsyncLineIterator {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__<'py>(slf: PyRef<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let state = Arc::clone(&slf.state);
        RUNTIME.future_into_py(
            slf.py(),
            async move {
                let mut s = state.lock().await;
                loop {
                    if !s.finished {
                        s.stream.check_open()?;
                    }
                    if let Some(line) = s.pending.pop_front() {
                        return Ok(line);
                    }
                    if s.finished {
                        return Err(pyo3::exceptions::PyStopAsyncIteration::new_err(()));
                    }
                    let chunk = s.stream.next_chunk().await?;
                    match chunk {
                        Some(src) => {
                            let text = s.decoder.decode(
                                &src, false,
                            );
                            let lines = s.lines.feed(&text);
                            s.pending.extend(lines);
                        }
                        None => {
                            // EOF: flush the byte decoder, feed the final text, then
                            // flush the line buffer — same two flushes as the sync path.
                            let text = s.decoder.decode(
                                &[],
                                true,
                            );
                            let lines = s.lines.feed(&text);
                            s.pending.extend(lines);
                            if let Some(last) = s.lines.flush() {
                                s.pending.push_back(last);
                            }
                            s.finished = true;
                        }
                    }
                }
            },
        )
    }
}

#[pyclass]
pub struct PyStreamResponse {
    pub parts: ResponseParts,
    pub(crate) body: Option<Body>,
    pub content_cache: PyOnceLock<Py<PyBytes>>,
    pub headers_cache: PyOnceLock<Py<PyHeaders>>,
}

#[pymethods]
impl PyStreamResponse {
    /// Drop the body: releases the connection, or the buffer if already read.
    /// An iterator still reading it raises on its next chunk.
    pub fn close(&mut self) {
        if let Some(body) = self.body.take() {
            body.close_blocking();
        }
    }

    /// Iterate over the body as the network delivers it, or in pieces of exactly
    /// `chunk_size` bytes with the remainder last.
    #[pyo3(signature = (chunk_size=None))]
    fn iter_bytes(&mut self, chunk_size: Option<usize>) -> PyResult<PyByteIterator> {
        let chunker = ByteChunker::new(Self::checked_chunk_size(chunk_size)?);
        let stream = self.start_stream()?;
        Ok(
            PyByteIterator {
                stream,
                chunker,
                finished: false,
            },
        )
    }

    /// Iterate over the decoded body as it arrives, or in pieces of exactly
    /// `chunk_size` characters with the remainder last.
    #[pyo3(signature = (chunk_size=None))]
    fn iter_text(&mut self, chunk_size: Option<usize>) -> PyResult<PyTextIterator> {
        let chunker = TextChunker::new(Self::checked_chunk_size(chunk_size)?);
        let stream = self.start_stream()?;
        Ok(
            PyTextIterator {
                stream,
                decoder: TextDecoder::new(self.parts.resolved_encoding()),
                chunker,
                finished: false,
            },
        )
    }

    /// Iterate over the decoded body line by line, terminators removed.
    fn iter_lines(&mut self) -> PyResult<PyLineIterator> {
        let stream = self.start_stream()?;
        Ok(
            PyLineIterator {
                stream,
                decoder: TextDecoder::new(self.parts.resolved_encoding()),
                lines: LineDecoder::default(),
                pending: VecDeque::new(),
                finished: false,
            },
        )
    }

    fn read(&mut self, py: Python<'_>) -> PyResult<Py<PyBytes>> {
        match self.body.take() {
            Some(Body::Live(response)) => {
                let bytes = py
                    .detach(|| RUNTIME.block_on(async { response.bytes().await }))?
                    .map_err(map_reqwest_error)?;
                self.body = Some(Body::Buffered(bytes));
            }
            Some(Body::Streaming(stream)) => {
                self.body = Some(Body::Streaming(stream));
                return Err(StreamConsumed::new_err("response already consumed"));
            }
            Some(buffered) => self.body = Some(buffered), // already Buffered — restore unchanged
            None => return Err(StreamClosed::new_err("response closed")),
        }
        self.content(py) // single, cached materialization — shared with the .content getter
    }

    #[getter]
    fn content(&self, py: Python<'_>) -> PyResult<Py<PyBytes>> {
        match &self.body {
            Some(Body::Buffered(bytes)) => Ok(
                self.content_cache
                    .get_or_init(
                        py,
                        || {
                            PyBytes::new(
                                py, bytes,
                            )
                            .unbind()
                        },
                    )
                    .clone_ref(py),
            ),
            Some(Body::Live(_) | Body::Streaming(_)) => {
                Err(ResponseNotRead::new_err("response not read; call read() first"))
            }
            None => Err(StreamClosed::new_err("response closed")),
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
            Some(Body::Live(_) | Body::Streaming(_)) => {
                Err(ResponseNotRead::new_err("response not read; call read() first"))
            }
            None => Err(StreamClosed::new_err("response closed")),
        }
    }

    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.body {
            Some(Body::Buffered(bytes)) => {
                let value = serde_json::from_slice(bytes).map_err(
                    |e| {
                        self.parts.json_decode_error(
                            bytes, &e,
                        )
                    },
                )?;
                value_to_py(
                    py, value,
                )
            }
            Some(Body::Live(_) | Body::Streaming(_)) => {
                Err(ResponseNotRead::new_err("response not read; call read() first"))
            }
            None => Err(StreamClosed::new_err("response closed")),
        }
    }

    #[getter]
    fn status_code(&self) -> u16 {
        self.parts.status_code
    }

    #[getter]
    fn headers(&self, py: Python<'_>) -> PyResult<Py<PyHeaders>> {
        // Materialized once and cached — sound because a response's headers are
        // read-only. Repeat access is then a refcount bump, and
        // `resp.headers is resp.headers` holds (matching httpx).
        self.headers_cache
            .get_or_try_init(
                py,
                || {
                    Py::new(
                        py,
                        PyHeaders::from_header_map(self.parts.headers.clone()),
                    )
                },
            )
            .map(|h| h.clone_ref(py))
    }

    #[getter]
    fn url(&self, py: Python<'_>) -> PyResult<Py<PyURL>> {
        self.parts.py_url(py)
    }

    #[getter]
    fn elapsed(&self) -> Duration {
        self.parts.elapsed
    }

    /// The response itself when the status is 2xx; otherwise HTTPStatusError with
    /// the response attached as `.response`.
    fn raise_for_status(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        let Some(error) = slf.borrow().parts.status_error() else {
            return Ok(slf);
        };
        error.value(slf.py()).setattr(
            "response", &slf,
        )?;
        Err(error)
    }

    #[getter]
    fn num_retries(&self) -> u32 {
        self.parts.num_retries
    }

    #[getter]
    fn retry_history(
        &self,
    ) -> &[(
        String,
        f64,
    )] {
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

    #[getter]
    fn is_closed(&self) -> bool {
        self.body.as_ref().is_none_or(Body::is_closed)
    }

    #[getter]
    fn is_consumed(&self) -> bool {
        !matches!(
            self.body,
            Some(Body::Live(_))
        )
    }
}

impl PyStreamResponse {
    fn checked_chunk_size(chunk_size: Option<usize>) -> PyResult<Option<usize>> {
        if chunk_size == Some(0) {
            return Err(PyValueError::new_err("chunk_size must be at least 1"));
        }
        Ok(chunk_size)
    }

    /// Hand the live body to an iterator, keeping a handle so `close` reaches it.
    fn start_stream(&mut self) -> PyResult<LiveStream> {
        match self.body.take() {
            Some(Body::Live(response)) => {
                let stream = LiveStream::new(response);
                self.body = Some(Body::Streaming(stream.clone()));
                Ok(stream)
            }
            Some(Body::Streaming(stream)) => {
                self.body = Some(Body::Streaming(stream));
                Err(StreamConsumed::new_err("response already consumed"))
            }
            Some(buffered) => {
                self.body = Some(buffered);
                Err(
                    StreamConsumed::new_err(
                        "response already read into memory; use .content, .text or .json()",
                    ),
                )
            }
            None => Err(StreamClosed::new_err("response closed")),
        }
    }

    pub fn from_pending(pending: PendingResponse) -> PyStreamResponse {
        let (parts, response) = pending.into_parts();
        PyStreamResponse {
            parts,
            body: Some(Body::Live(response)),
            content_cache: PyOnceLock::new(),
            headers_cache: PyOnceLock::new(),
        }
    }
}

#[pyclass]
pub struct PyAsyncStreamResponse {
    pub parts: ResponseParts,
    // Shared + interior-mutable so `aread`'s future can store the buffered bytes
    // back onto self *after* the await (it can't hold `&mut self` across it).
    // A std Mutex is enough: every critical section is a tiny take/store that's
    // never held across an await or a GIL acquisition.
    pub(crate) body: Arc<Mutex<Option<Body>>>,
    pub content_cache: PyOnceLock<Py<PyBytes>>,
    pub headers_cache: PyOnceLock<Py<PyHeaders>>,
}

#[pymethods]
impl PyAsyncStreamResponse {
    /// Iterate over the body as the network delivers it, or in pieces of exactly
    /// `chunk_size` bytes with the remainder last.
    #[pyo3(signature = (chunk_size=None))]
    fn aiter_bytes(&mut self, chunk_size: Option<usize>) -> PyResult<PyAsyncByteIterator> {
        let chunker = ByteChunker::new(PyStreamResponse::checked_chunk_size(chunk_size)?);
        let stream = self.start_stream()?;
        Ok(
            PyAsyncByteIterator {
                state: Arc::new(
                    TokioMutex::new(
                        AsyncByteState {
                            stream,
                            chunker,
                            finished: false,
                        },
                    ),
                ),
            },
        )
    }

    /// Iterate over the decoded body as it arrives, or in pieces of exactly
    /// `chunk_size` characters with the remainder last.
    #[pyo3(signature = (chunk_size=None))]
    fn aiter_text(&mut self, chunk_size: Option<usize>) -> PyResult<PyAsyncTextIterator> {
        let chunker = TextChunker::new(PyStreamResponse::checked_chunk_size(chunk_size)?);
        let stream = self.start_stream()?;
        Ok(
            PyAsyncTextIterator {
                state: Arc::new(
                    TokioMutex::new(
                        AsyncTextState {
                            stream,
                            decoder: TextDecoder::new(self.parts.resolved_encoding()),
                            chunker,
                            finished: false,
                        },
                    ),
                ),
            },
        )
    }

    /// Iterate over the decoded body line by line, terminators removed.
    fn aiter_lines(&mut self) -> PyResult<PyAsyncLineIterator> {
        let stream = self.start_stream()?;
        Ok(
            PyAsyncLineIterator {
                state: Arc::new(
                    TokioMutex::new(
                        AsyncLineState {
                            stream,
                            decoder: TextDecoder::new(self.parts.resolved_encoding()),
                            lines: LineDecoder::default(),
                            pending: VecDeque::new(),
                            finished: false,
                        },
                    ),
                ),
            },
        )
    }

    /// Read the entire remaining body into memory and return it as `bytes`.
    /// Buffers in place, so `.content`/`.text`/`.json()` work afterward.
    fn aread<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let body = Arc::clone(&self.body);
        RUNTIME.future_into_py(
            py,
            async move {
                // Take the live handle under the lock, but never hold the lock
                // across the await or a GIL acquisition.
                let live = {
                    let mut guard = body.lock().unwrap();
                    match guard.take() {
                        Some(Body::Live(r)) => r,
                        Some(Body::Buffered(b)) => {
                            // Already read — restore and return the same bytes.
                            *guard = Some(Body::Buffered(b.clone()));
                            drop(guard);
                            return Python::attach(
                                |py| {
                                    Ok(
                                        PyBytes::new(
                                            py, &b,
                                        )
                                        .unbind(),
                                    )
                                },
                            );
                        }
                        Some(Body::Streaming(stream)) => {
                            *guard = Some(Body::Streaming(stream));
                            return Err(StreamConsumed::new_err("response already consumed"));
                        }
                        None => {
                            return Err(StreamClosed::new_err("response closed"));
                        }
                    }
                };
                let bytes = live.bytes().await.map_err(map_reqwest_error)?;
                *body.lock().unwrap() = Some(Body::Buffered(bytes.clone()));
                Python::attach(
                    |py| {
                        Ok(
                            PyBytes::new(
                                py, &bytes,
                            )
                            .unbind(),
                        )
                    },
                )
            },
        )
    }

    /// Release the connection (or drop the buffer) early. An iterator still
    /// reading it raises on its next chunk.
    fn aclose<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let body = Arc::clone(&self.body);
        RUNTIME.future_into_py(
            py,
            async move {
                let taken = body.lock().unwrap().take();
                if let Some(taken) = taken {
                    taken.close().await;
                }
                Ok(())
            },
        )
    }

    #[getter]
    fn content(&self, py: Python<'_>) -> PyResult<Py<PyBytes>> {
        let bytes = match &*self.body.lock().unwrap() {
            Some(Body::Buffered(b)) => b.clone(),
            Some(Body::Live(_) | Body::Streaming(_)) => {
                return Err(ResponseNotRead::new_err("response not read; call aread() first"));
            }
            None => return Err(StreamClosed::new_err("response closed")),
        };
        Ok(
            self.content_cache
                .get_or_init(
                    py,
                    || {
                        PyBytes::new(
                            py, &bytes,
                        )
                        .unbind()
                    },
                )
                .clone_ref(py),
        )
    }

    #[getter]
    fn text(&self) -> PyResult<String> {
        let bytes = match &*self.body.lock().unwrap() {
            Some(Body::Buffered(b)) => b.clone(),
            Some(Body::Live(_) | Body::Streaming(_)) => {
                return Err(ResponseNotRead::new_err("response not read; call aread() first"));
            }
            None => return Err(StreamClosed::new_err("response closed")),
        };
        let (decoded, _, _) = self.parts.resolved_encoding().decode(&bytes);
        Ok(decoded.into_owned())
    }

    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let bytes = match &*self.body.lock().unwrap() {
            Some(Body::Buffered(b)) => b.clone(),
            Some(Body::Live(_) | Body::Streaming(_)) => {
                return Err(ResponseNotRead::new_err("response not read; call aread() first"));
            }
            None => return Err(StreamClosed::new_err("response closed")),
        };
        let value = serde_json::from_slice(&bytes).map_err(
            |e| {
                self.parts.json_decode_error(
                    &bytes, &e,
                )
            },
        )?;
        value_to_py(
            py, value,
        )
    }

    #[getter]
    fn status_code(&self) -> u16 {
        self.parts.status_code
    }

    #[getter]
    fn headers(&self, py: Python<'_>) -> PyResult<Py<PyHeaders>> {
        // Materialized once and cached — sound because a response's headers are
        // read-only. Repeat access is then a refcount bump, and
        // `resp.headers is resp.headers` holds (matching httpx).
        self.headers_cache
            .get_or_try_init(
                py,
                || {
                    Py::new(
                        py,
                        PyHeaders::from_header_map(self.parts.headers.clone()),
                    )
                },
            )
            .map(|h| h.clone_ref(py))
    }

    #[getter]
    fn url(&self, py: Python<'_>) -> PyResult<Py<PyURL>> {
        self.parts.py_url(py)
    }

    #[getter]
    fn elapsed(&self) -> Duration {
        self.parts.elapsed
    }

    /// The response itself when the status is 2xx; otherwise HTTPStatusError with
    /// the response attached as `.response`.
    fn raise_for_status(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        let Some(error) = slf.borrow().parts.status_error() else {
            return Ok(slf);
        };
        error.value(slf.py()).setattr(
            "response", &slf,
        )?;
        Err(error)
    }

    #[getter]
    fn num_retries(&self) -> u32 {
        self.parts.num_retries
    }

    #[getter]
    fn retry_history(
        &self,
    ) -> &[(
        String,
        f64,
    )] {
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

    #[getter]
    fn is_closed(&self) -> bool {
        self.body
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(Body::is_closed)
    }

    #[getter]
    fn is_consumed(&self) -> bool {
        !matches!(
            *self.body.lock().unwrap(),
            Some(Body::Live(_))
        )
    }
}

impl PyAsyncStreamResponse {
    /// Take the body out for closing from Rust; the caller awaits `Body::close`.
    pub fn take_body(&self) -> Option<Body> {
        self.body.lock().unwrap().take()
    }

    /// Hand the live body to an iterator, keeping a handle so closing reaches it.
    fn start_stream(&self) -> PyResult<LiveStream> {
        let mut guard = self.body.lock().unwrap();
        match guard.take() {
            Some(Body::Live(response)) => {
                let stream = LiveStream::new(response);
                *guard = Some(Body::Streaming(stream.clone()));
                Ok(stream)
            }
            Some(Body::Streaming(stream)) => {
                *guard = Some(Body::Streaming(stream));
                Err(StreamConsumed::new_err("response already consumed"))
            }
            Some(buffered) => {
                *guard = Some(buffered);
                Err(
                    StreamConsumed::new_err(
                        "response already read into memory; use .content, .text or .json()",
                    ),
                )
            }
            None => Err(StreamClosed::new_err("response closed")),
        }
    }

    pub fn from_pending(pending: PendingResponse) -> PyAsyncStreamResponse {
        let (parts, response) = pending.into_parts();
        PyAsyncStreamResponse {
            parts,
            body: Arc::new(Mutex::new(Some(Body::Live(response)))),
            content_cache: PyOnceLock::new(),
            headers_cache: PyOnceLock::new(),
        }
    }
}
