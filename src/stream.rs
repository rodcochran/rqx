use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use encoding_rs::{Decoder, Encoding};
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
    decoder: TextDecoder,
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

        loop {
            let chunk = py.detach(|| {
                RUNTIME
                    .get()
                    .expect("runtime not initialized")
                    .block_on(async {
                        // .lock().await on TokioMutex yields on contention
                        // instead of blocking the OS thread.
                        let mut guard = stream.lock().await;
                        guard.as_mut().next().await.transpose()
                    })
            });

            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => return Err(RqxError::new_err(format!("stream error: {e}"))),
            };

            match chunk {
                // A chunk may complete no character (the decoder holds the
                // partial bytes), so loop until decoding yields some text.
                Some(src) => {
                    let text = slf.decoder.decode(&src, false);
                    if !text.is_empty() {
                        return Ok(Some(text));
                    }
                }
                // End of stream — flush any character the decoder still holds.
                None => {
                    slf.finished = true;
                    let text = slf.decoder.decode(&[], true);
                    return Ok((!text.is_empty()).then_some(text));
                }
            }
        }
    }
}

#[pyclass]
struct PyLineIterator {
    stream: Arc<TokioMutex<ChunkStream>>,
    #[allow(dead_code)]
    chunk_size: u32,
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
        let stream = Arc::clone(&slf.stream);

        loop {
            // Drain already-decoded lines before touching the network.
            if let Some(line) = slf.pending.pop_front() {
                return Ok(Some(line));
            }
            if slf.finished {
                return Ok(None);
            }

            let chunk = py.detach(|| {
                RUNTIME
                    .get()
                    .expect("runtime not initialized")
                    .block_on(async {
                        let mut guard = stream.lock().await;
                        guard.as_mut().next().await.transpose()
                    })
            });

            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => return Err(RqxError::new_err(format!("stream error: {e}"))),
            };

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
        self.close();
    }

    fn close(&mut self) {
        // drops the response, closes connection
        self.body = None;
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

        Ok(PyTextIterator {
            stream: Arc::new(TokioMutex::new(Box::pin(response.bytes_stream()))),
            chunk_size,
            decoder: TextDecoder::new(self.parts.resolved_encoding()),
            finished: false,
        })
    }

    #[pyo3(signature = (chunk_size=8192))]
    fn iter_lines(&mut self, chunk_size: u32) -> PyResult<PyLineIterator> {
        let response = match self.body.take() {
            Some(Body::Live(r)) => r,
            Some(other) => {
                self.body = Some(other);
                return Err(RqxError::new_err("response already read into memory"));
            }
            None => return Err(RqxError::new_err("response already consumed or closed")),
        };
        Ok(PyLineIterator {
            stream: Arc::new(TokioMutex::new(Box::pin(response.bytes_stream()))),
            chunk_size,
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

    #[getter]
    fn is_closed(&self) -> bool {
        self.body.is_none()
    }

    #[getter]
    fn is_consumed(&self) -> bool {
        !matches!(self.body, Some(Body::Live(_)))
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
    pub content_cache: PyOnceLock<Py<PyBytes>>,
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
            content_cache: PyOnceLock::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::LineDecoder;

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
