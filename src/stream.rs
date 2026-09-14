use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bytes::{Bytes, BytesMut};
use encoding_rs::{Decoder, Encoding};
use futures::{Stream, StreamExt};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::{Py, PyAny, PyRef, PyRefMut, PyResult, Python, pyclass, pymethods};
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

/// Streaming HTTP body source. `Pin<Box<dyn ...>>` is standard practice for
/// storing an erased, async-trait-object Stream: `dyn Stream` is unsized
/// (hence Box), the stream internally self-references its connection state
/// so it must not move once polled (hence Pin), and `+ Send` lets it cross
/// thread boundaries when shared via `Arc`.
type ChunkStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

/// The body while an iterator reads it. The response holds one handle and the
/// iterator another, so closing the response ends the iterator too, including
/// a read that is waiting on the network.
#[derive(Clone)]
pub struct LiveStream(Arc<LiveStreamInner>);

struct LiveStreamInner {
    stream: TokioMutex<Option<ChunkStream>>,
    closed: AtomicBool,
    // Wakes a read that is waiting on the network when the response is closed.
    close_signal: Notify,
}

impl LiveStream {
    fn new(response: reqwest::Response) -> Self {
        Self(Arc::new(LiveStreamInner {
            stream: TokioMutex::new(Some(Box::pin(response.bytes_stream()))),
            closed: AtomicBool::new(false),
            close_signal: Notify::new(),
        }))
    }

    /// The next chunk, `None` at the end. A closed response or a failed read
    /// drops the stream, so the connection is released without waiting for
    /// the iterator to be dropped.
    async fn next_chunk(&self) -> PyResult<Option<Bytes>> {
        let mut slot = self.0.stream.lock().await;
        self.check_open()?;
        let Some(stream) = slot.as_mut() else {
            return Err(RqxError::new_err("response closed"));
        };
        let next = tokio::select! {
            biased;
            _ = self.0.close_signal.notified() => {
                *slot = None;
                return Err(RqxError::new_err("response closed"));
            }
            next = stream.next() => next,
        };
        match next {
            Some(Ok(bytes)) => Ok(Some(bytes)),
            Some(Err(e)) => {
                *slot = None;
                Err(map_reqwest_error(e))
            }
            None => {
                *slot = None;
                Ok(None)
            }
        }
    }

    /// Mark closed and wake a pending read, then drop the stream. The flag and
    /// the wake-up come first so a read waiting on the network gives up the
    /// lock instead of holding this call until data arrives.
    async fn close(&self) {
        self.mark_closed();
        *self.0.stream.lock().await = None;
    }

    /// For the sync response, which closes from the Python thread, off the runtime.
    fn close_blocking(&self) {
        self.mark_closed();
        *self.0.stream.blocking_lock() = None;
    }

    fn mark_closed(&self) {
        self.0.closed.store(true, Ordering::Release);
        self.0.close_signal.notify_one();
    }

    /// Raise if the response was closed under the iterator; buffered pieces are
    /// not served after a close, only after the stream's own end.
    fn check_open(&self) -> PyResult<()> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(RqxError::new_err("response closed"));
        }
        Ok(())
    }

    /// Closed, or read to the end. A poll in flight holds the lock; that counts as open.
    fn is_closed(&self) -> bool {
        self.0.closed.load(Ordering::Acquire)
            || self
                .0
                .stream
                .try_lock()
                .map(|slot| slot.is_none())
                .unwrap_or(false)
    }
}

/// A streaming text decoder: an `encoding_rs::Decoder` plus the capacity
/// handling its `decode_to_string` requires (that method writes into the
/// String's existing spare capacity and returns `OutputFull`, writing nothing,
/// if there's none — it does NOT grow the String). Held by the text and line
/// iterators so both decode identically.
struct TextDecoder(Decoder);

impl TextDecoder {
    fn new(encoding: &'static Encoding) -> Self {
        Self(encoding.new_decoder())
    }

    /// Decode one chunk of bytes to text. `last` flushes any partial character
    /// the decoder is holding at end of stream.
    fn decode(&mut self, src: &[u8], last: bool) -> String {
        let mut out = String::new();
        if let Some(needed) = self.0.max_utf8_buffer_length(src.len()) {
            out.reserve(needed);
        }
        // Reserved worst-case capacity above, so this consumes all of `src` in
        // one call; the (CoderResult, read, replaced) tuple isn't needed.
        let _ = self.0.decode_to_string(src, &mut out, last);
        out
    }
}

/// Splits a stream of decoded text into lines, reassembling lines that span
/// chunk boundaries. Port of httpx's `LineDecoder`. Pure — no I/O, no pyo3 — so
/// the cross-chunk behavior is unit-testable with hand-fed `&str` chunks.
#[derive(Default)]
struct LineDecoder {
    /// The partial trailing line carried across `feed` calls.
    buffer: String,
    /// A trailing `\r` deferred to the next `feed`, so a `\r\n` split across a
    /// chunk boundary isn't mistaken for two separate line endings.
    trailing_cr: bool,
}

impl LineDecoder {
    /// Characters Python's `str.splitlines()` treats as line boundaries.
    /// Mirrored so `iter_lines` matches httpx, including SSE's lone `\r`.
    fn is_line_break(c: char) -> bool {
        matches!(
            c,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        )
    }

    /// Equivalent of Python `str.splitlines()`: split on the line-break set,
    /// stripping terminators, with `\r\n` treated as a single break and no
    /// trailing empty segment after a final terminator.
    fn split_lines(text: &str) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next(); // consume the LF of a CRLF
                }
                lines.push(std::mem::take(&mut current));
            } else if Self::is_line_break(c) {
                lines.push(std::mem::take(&mut current));
            } else {
                current.push(c);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }

    fn feed(&mut self, text: &str) -> Vec<String> {
        let mut text = if self.trailing_cr {
            self.trailing_cr = false;
            format!("\r{text}")
        } else {
            text.to_string()
        };

        if text.ends_with('\r') {
            self.trailing_cr = true;
            text.pop();
        }

        if text.is_empty() {
            return Vec::new();
        }

        let trailing_newline = text.chars().next_back().is_some_and(Self::is_line_break);
        let mut lines = Self::split_lines(&text);

        // A single unterminated segment is just more of the partial line.
        if lines.len() == 1 && !trailing_newline {
            self.buffer.push_str(&lines[0]);
            return Vec::new();
        }

        // Any buffered partial line is the start of this chunk's first segment.
        if !self.buffer.is_empty() {
            lines[0] = format!("{}{}", self.buffer, lines[0]);
            self.buffer.clear();
        }

        // A non-newline-terminated tail becomes the next partial line.
        if !trailing_newline {
            self.buffer = lines.pop().unwrap();
        }

        lines
    }

    /// Emit the final partial line at end of stream, if any.
    fn flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() && !self.trailing_cr {
            return None;
        }
        self.trailing_cr = false;
        Some(std::mem::take(&mut self.buffer))
    }
}

/// Regroups network chunks into pieces of exactly `size` bytes; the last piece
/// is whatever remains. Whole pieces are split off the incoming chunk without
/// copying; only the bytes needed to complete a piece are copied into `carry`,
/// one reused buffer, so memory stays at about `size` plus one network chunk.
struct ByteChunker {
    size: usize,
    carry: BytesMut,
    head: Bytes,
}

impl ByteChunker {
    fn new(size: usize) -> Self {
        Self {
            size,
            carry: BytesMut::new(),
            head: Bytes::new(),
        }
    }

    fn feed(&mut self, mut bytes: Bytes) {
        if !self.carry.is_empty() {
            let take = (self.size - self.carry.len()).min(bytes.len());
            self.carry.extend_from_slice(&bytes.split_to(take));
        }
        self.head = bytes;
    }

    /// The next full piece, if one is buffered.
    fn next_full(&mut self) -> Option<Bytes> {
        if self.carry.len() >= self.size {
            return Some(self.carry.split_to(self.size).freeze());
        }
        if self.head.len() >= self.size && self.carry.is_empty() {
            return Some(self.head.split_to(self.size));
        }
        // Too short on both sides: park the head so the next chunk completes it.
        if !self.head.is_empty() {
            self.carry.extend_from_slice(&self.head);
            self.head = Bytes::new();
        }
        None
    }

    /// Whatever is left once the stream has ended.
    fn flush(&mut self) -> Option<Bytes> {
        self.carry.extend_from_slice(&self.head);
        self.head = Bytes::new();
        (!self.carry.is_empty()).then(|| self.carry.split().freeze())
    }
}

/// Regroups decoded text into pieces of exactly `size` characters; the last
/// piece is whatever remains. A piece never splits a character. Consumed text
/// is dropped once per `feed`, not per piece, so small sizes stay linear.
struct TextChunker {
    size: usize,
    pending: String,
    consumed: usize,
}

impl TextChunker {
    fn new(size: usize) -> Self {
        Self {
            size,
            pending: String::new(),
            consumed: 0,
        }
    }

    fn feed(&mut self, text: &str) {
        if self.consumed > 0 {
            self.pending.drain(..self.consumed);
            self.consumed = 0;
        }
        self.pending.push_str(text);
    }

    /// The next full piece, if `size` characters are buffered.
    fn next_full(&mut self) -> Option<String> {
        let unread = &self.pending[self.consumed..];
        let (last_start, last) = unread.char_indices().nth(self.size - 1)?;
        let end = last_start + last.len_utf8();
        let piece = unread[..end].to_string();
        self.consumed += end;
        Some(piece)
    }

    fn flush(&mut self) -> Option<String> {
        let rest = self.pending[self.consumed..].to_string();
        self.pending.clear();
        self.consumed = 0;
        (!rest.is_empty()).then_some(rest)
    }
}

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
                return Ok(Some(PyBytes::new(py, &piece).unbind()));
            }
            if slf.finished {
                return Ok(slf
                    .chunker
                    .flush()
                    .map(|piece| PyBytes::new(py, &piece).unbind()));
            }
            match block_on_inner(py, stream.next_chunk())? {
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
            match block_on_inner(py, stream.next_chunk())? {
                Some(src) => {
                    let text = slf.decoder.decode(&src, false);
                    slf.chunker.feed(&text);
                }
                // End of stream: flush any character the decoder still holds.
                None => {
                    slf.finished = true;
                    let text = slf.decoder.decode(&[], true);
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

            let chunk = block_on_inner(py, stream.next_chunk())?;

            match chunk {
                Some(src) => {
                    let text = slf.decoder.decode(&src, false);
                    let lines = slf.lines.feed(&text);
                    slf.pending.extend(lines);
                }
                None => {
                    // End of stream: flush the byte decoder, feed any final
                    // text through the line splitter, THEN flush the line
                    // buffer. Both flushes are required, in this order.
                    let text = slf.decoder.decode(&[], true);
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
        Ok(PyBytes::new(py, &self.0))
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
        RUNTIME.future_into_py(slf.py(), async move {
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
        })
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
        RUNTIME.future_into_py(slf.py(), async move {
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
                        let text = s.decoder.decode(&src, false);
                        s.chunker.feed(&text);
                    }
                    None => {
                        s.finished = true;
                        let text = s.decoder.decode(&[], true);
                        s.chunker.feed(&text);
                    }
                }
            }
        })
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
        RUNTIME.future_into_py(slf.py(), async move {
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
                        let text = s.decoder.decode(&src, false);
                        let lines = s.lines.feed(&text);
                        s.pending.extend(lines);
                    }
                    None => {
                        // EOF: flush the byte decoder, feed the final text, then
                        // flush the line buffer — same two flushes as the sync path.
                        let text = s.decoder.decode(&[], true);
                        let lines = s.lines.feed(&text);
                        s.pending.extend(lines);
                        if let Some(last) = s.lines.flush() {
                            s.pending.push_back(last);
                        }
                        s.finished = true;
                    }
                }
            }
        })
    }
}

/*
Response object
*/

pub enum Body {
    Live(reqwest::Response),
    Streaming(LiveStream),
    Buffered(Bytes),
}

impl Body {
    /// Release whatever the body holds; a stream being read is ended for its iterator too.
    pub async fn close(self) {
        if let Body::Streaming(stream) = self {
            stream.close().await;
        }
    }

    fn close_blocking(self) {
        if let Body::Streaming(stream) = self {
            stream.close_blocking();
        }
    }

    fn is_closed(&self) -> bool {
        match self {
            Body::Streaming(stream) => stream.is_closed(),
            Body::Live(_) | Body::Buffered(_) => false,
        }
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

    /// Iterate over the body in pieces of exactly `chunk_size` bytes; the last
    /// piece is whatever remains.
    #[pyo3(signature = (chunk_size=8192))]
    fn iter_bytes(&mut self, chunk_size: usize) -> PyResult<PyByteIterator> {
        let chunker = ByteChunker::new(Self::checked_chunk_size(chunk_size)?);
        let stream = self.start_stream()?;
        Ok(PyByteIterator {
            stream,
            chunker,
            finished: false,
        })
    }

    /// Iterate over the decoded body in pieces of exactly `chunk_size`
    /// characters; the last piece is whatever remains.
    #[pyo3(signature = (chunk_size=8192))]
    fn iter_text(&mut self, chunk_size: usize) -> PyResult<PyTextIterator> {
        let chunker = TextChunker::new(Self::checked_chunk_size(chunk_size)?);
        let stream = self.start_stream()?;
        Ok(PyTextIterator {
            stream,
            decoder: TextDecoder::new(self.parts.resolved_encoding()),
            chunker,
            finished: false,
        })
    }

    /// Iterate over the decoded body line by line, terminators removed.
    fn iter_lines(&mut self) -> PyResult<PyLineIterator> {
        let stream = self.start_stream()?;
        Ok(PyLineIterator {
            stream,
            decoder: TextDecoder::new(self.parts.resolved_encoding()),
            lines: LineDecoder::default(),
            pending: VecDeque::new(),
            finished: false,
        })
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
                return Err(RqxError::new_err("response already consumed"));
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
            Some(Body::Live(_) | Body::Streaming(_)) => {
                Err(RqxError::new_err("response not read; call read() first"))
            }
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
            Some(Body::Live(_) | Body::Streaming(_)) => {
                Err(RqxError::new_err("response not read; call read() first"))
            }
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
            Some(Body::Live(_) | Body::Streaming(_)) => {
                Err(RqxError::new_err("response not read; call read() first"))
            }
            None => Err(RqxError::new_err("response consumed or closed")),
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
            .get_or_try_init(py, || {
                Py::new(py, PyHeaders::from_header_map(self.parts.headers.clone()))
            })
            .map(|h| h.clone_ref(py))
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

    #[getter]
    fn is_closed(&self) -> bool {
        self.body.as_ref().is_none_or(Body::is_closed)
    }

    #[getter]
    fn is_consumed(&self) -> bool {
        !matches!(self.body, Some(Body::Live(_)))
    }
}

impl PyStreamResponse {
    fn checked_chunk_size(chunk_size: usize) -> PyResult<usize> {
        if chunk_size == 0 {
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
                Err(RqxError::new_err("response already consumed"))
            }
            Some(buffered) => {
                self.body = Some(buffered);
                Err(RqxError::new_err("response already read into memory"))
            }
            None => Err(RqxError::new_err("response already consumed or closed")),
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
    /// Iterate over the body in pieces of exactly `chunk_size` bytes; the last
    /// piece is whatever remains.
    #[pyo3(signature = (chunk_size=8192))]
    fn aiter_bytes(&mut self, chunk_size: usize) -> PyResult<PyAsyncByteIterator> {
        let chunker = ByteChunker::new(PyStreamResponse::checked_chunk_size(chunk_size)?);
        let stream = self.start_stream()?;
        Ok(PyAsyncByteIterator {
            state: Arc::new(TokioMutex::new(AsyncByteState {
                stream,
                chunker,
                finished: false,
            })),
        })
    }

    /// Iterate over the decoded body in pieces of exactly `chunk_size`
    /// characters; the last piece is whatever remains.
    #[pyo3(signature = (chunk_size=8192))]
    fn aiter_text(&mut self, chunk_size: usize) -> PyResult<PyAsyncTextIterator> {
        let chunker = TextChunker::new(PyStreamResponse::checked_chunk_size(chunk_size)?);
        let stream = self.start_stream()?;
        Ok(PyAsyncTextIterator {
            state: Arc::new(TokioMutex::new(AsyncTextState {
                stream,
                decoder: TextDecoder::new(self.parts.resolved_encoding()),
                chunker,
                finished: false,
            })),
        })
    }

    /// Iterate over the decoded body line by line, terminators removed.
    fn aiter_lines(&mut self) -> PyResult<PyAsyncLineIterator> {
        let stream = self.start_stream()?;
        Ok(PyAsyncLineIterator {
            state: Arc::new(TokioMutex::new(AsyncLineState {
                stream,
                decoder: TextDecoder::new(self.parts.resolved_encoding()),
                lines: LineDecoder::default(),
                pending: VecDeque::new(),
                finished: false,
            })),
        })
    }

    /// Read the entire remaining body into memory and return it as `bytes`.
    /// Buffers in place, so `.content`/`.text`/`.json()` work afterward.
    fn aread<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let body = Arc::clone(&self.body);
        RUNTIME.future_into_py(py, async move {
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
                        return Python::attach(|py| Ok(PyBytes::new(py, &b).unbind()));
                    }
                    Some(Body::Streaming(stream)) => {
                        *guard = Some(Body::Streaming(stream));
                        return Err(RqxError::new_err("response already consumed"));
                    }
                    None => {
                        return Err(RqxError::new_err("response already consumed or closed"));
                    }
                }
            };
            let bytes = live.bytes().await.map_err(map_reqwest_error)?;
            *body.lock().unwrap() = Some(Body::Buffered(bytes.clone()));
            Python::attach(|py| Ok(PyBytes::new(py, &bytes).unbind()))
        })
    }

    /// Release the connection (or drop the buffer) early. An iterator still
    /// reading it raises on its next chunk.
    fn aclose<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let body = Arc::clone(&self.body);
        RUNTIME.future_into_py(py, async move {
            let taken = body.lock().unwrap().take();
            if let Some(taken) = taken {
                taken.close().await;
            }
            Ok(())
        })
    }

    #[getter]
    fn content(&self, py: Python<'_>) -> PyResult<Py<PyBytes>> {
        let bytes = match &*self.body.lock().unwrap() {
            Some(Body::Buffered(b)) => b.clone(),
            Some(Body::Live(_) | Body::Streaming(_)) => {
                return Err(RqxError::new_err("response not read; call aread() first"));
            }
            None => return Err(RqxError::new_err("response consumed or closed")),
        };
        Ok(self
            .content_cache
            .get_or_init(py, || PyBytes::new(py, &bytes).unbind())
            .clone_ref(py))
    }

    #[getter]
    fn text(&self) -> PyResult<String> {
        let bytes = match &*self.body.lock().unwrap() {
            Some(Body::Buffered(b)) => b.clone(),
            Some(Body::Live(_) | Body::Streaming(_)) => {
                return Err(RqxError::new_err("response not read; call aread() first"));
            }
            None => return Err(RqxError::new_err("response consumed or closed")),
        };
        let (decoded, _, _) = self.parts.resolved_encoding().decode(&bytes);
        Ok(decoded.into_owned())
    }

    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let bytes = match &*self.body.lock().unwrap() {
            Some(Body::Buffered(b)) => b.clone(),
            Some(Body::Live(_) | Body::Streaming(_)) => {
                return Err(RqxError::new_err("response not read; call aread() first"));
            }
            None => return Err(RqxError::new_err("response consumed or closed")),
        };
        let value = match serde_json::from_slice(&bytes) {
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
            .get_or_try_init(py, || {
                Py::new(py, PyHeaders::from_header_map(self.parts.headers.clone()))
            })
            .map(|h| h.clone_ref(py))
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
        !matches!(*self.body.lock().unwrap(), Some(Body::Live(_)))
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
                Err(RqxError::new_err("response already consumed"))
            }
            Some(buffered) => {
                *guard = Some(buffered);
                Err(RqxError::new_err("response already read into memory"))
            }
            None => Err(RqxError::new_err("response already consumed or closed")),
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

#[cfg(test)]
mod tests {
    use super::{ByteChunker, LineDecoder, TextChunker};
    use bytes::Bytes;

    /// Feed `chunks` through a chunker of `size` and collect what it yields.
    fn rechunk(size: usize, chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut chunker = ByteChunker::new(size);
        let mut out = Vec::new();
        for chunk in chunks {
            chunker.feed(Bytes::copy_from_slice(chunk));
            while let Some(piece) = chunker.next_full() {
                out.push(piece.to_vec());
            }
        }
        if let Some(rest) = chunker.flush() {
            out.push(rest.to_vec());
        }
        out
    }

    #[test]
    fn byte_chunker_yields_exact_pieces_and_a_remainder() {
        let body: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        for size in [1usize, 3, 7, 64, 333, 999, 1000, 1001] {
            for cut in [1usize, 5, 128, 512, 1000] {
                let chunks: Vec<&[u8]> = body.chunks(cut).collect();
                let pieces = rechunk(size, &chunks);
                assert!(
                    pieces[..pieces.len() - 1].iter().all(|p| p.len() == size),
                    "size {size} cut {cut}"
                );
                assert!(!pieces.last().unwrap().is_empty() && pieces.last().unwrap().len() <= size);
                assert_eq!(pieces.concat(), body, "size {size} cut {cut}");
            }
        }
    }

    #[test]
    fn byte_chunker_empty_body_yields_nothing() {
        assert!(rechunk(16, &[]).is_empty());
        assert!(rechunk(16, &[b""]).is_empty());
    }

    #[test]
    fn text_chunker_counts_characters() {
        let mut chunker = TextChunker::new(2);
        chunker.feed("aé€🙂b");
        assert_eq!(chunker.next_full().as_deref(), Some("aé"));
        assert_eq!(chunker.next_full().as_deref(), Some("€🙂"));
        assert_eq!(chunker.next_full(), None);
        assert_eq!(chunker.flush().as_deref(), Some("b"));
        assert_eq!(chunker.flush(), None);
    }

    #[test]
    fn text_chunker_yields_a_piece_of_exactly_size_without_waiting_for_more() {
        // A live stream that sends exactly `size` characters and pauses must not stall.
        let mut chunker = TextChunker::new(3);
        chunker.feed("a€🙂");
        assert_eq!(chunker.next_full().as_deref(), Some("a€🙂"));
        assert_eq!(chunker.next_full(), None);
        chunker.feed("bc");
        assert_eq!(chunker.next_full(), None);
        chunker.feed("d");
        assert_eq!(chunker.next_full().as_deref(), Some("bcd"));
        assert_eq!(chunker.flush(), None);
    }

    #[test]
    fn split_lines_matches_splitlines() {
        assert_eq!(LineDecoder::split_lines("a\nb"), ["a", "b"]);
        assert_eq!(LineDecoder::split_lines("a\n"), ["a"]); // no trailing empty after a terminator
        assert_eq!(LineDecoder::split_lines("a\n\n"), ["a", ""]);
        assert!(LineDecoder::split_lines("").is_empty());
        assert_eq!(LineDecoder::split_lines("a\r\nb"), ["a", "b"]); // CRLF is a single break
        assert_eq!(LineDecoder::split_lines("a\rb"), ["a", "b"]); // lone CR is a break
    }

    #[test]
    fn feed_emits_complete_lines() {
        let mut d = LineDecoder::default();
        assert_eq!(d.feed("a\nb\nc\n"), ["a", "b", "c"]);
    }

    #[test]
    fn feed_buffers_partial_line_across_chunks() {
        let mut d = LineDecoder::default();
        assert!(d.feed("ab").is_empty()); // unterminated — buffered, nothing yet
        assert_eq!(d.feed("cd\n"), ["abcd"]); // completed by the next chunk
    }

    #[test]
    fn feed_reassembles_crlf_split_across_chunks() {
        // The case we can't force over a socket: "\r\n" straddles the boundary.
        // The trailing "\r" must be deferred, not emitted as a lone-CR line.
        let mut d = LineDecoder::default();
        assert!(d.feed("a\r").is_empty()); // trailing CR deferred
        assert_eq!(d.feed("\nb\n"), ["a", "b"]); // no spurious empty line
    }

    #[test]
    fn feed_treats_lone_cr_as_terminator() {
        let mut d = LineDecoder::default();
        assert_eq!(d.feed("a\rb\r"), ["a"]); // "b" deferred (its own trailing CR)
        assert_eq!(d.flush(), Some("b".to_string()));
    }

    #[test]
    fn flush_emits_final_unterminated_line() {
        let mut d = LineDecoder::default();
        assert!(d.feed("last line").is_empty());
        assert_eq!(d.flush(), Some("last line".to_string()));
        assert_eq!(d.flush(), None); // nothing left
    }
}
