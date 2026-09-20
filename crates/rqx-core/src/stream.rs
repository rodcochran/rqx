use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::{Bytes, BytesMut};
use encoding_rs::{Decoder, Encoding};
use futures::{Stream, StreamExt};

use tokio::sync::Mutex as TokioMutex;
use tokio::sync::Notify;

use crate::error::RqxError;

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
    pub fn new(response: reqwest::Response) -> Self {
        Self(
            Arc::new(
                LiveStreamInner {
                    stream: TokioMutex::new(Some(Box::pin(response.bytes_stream()))),
                    closed: AtomicBool::new(false),
                    close_signal: Notify::new(),
                },
            ),
        )
    }

    /// The next chunk, `None` at the end. A closed response or a failed read
    /// drops the stream, so the connection is released without waiting for
    /// the iterator to be dropped.
    pub async fn next_chunk(&self) -> Result<Option<Bytes>, RqxError> {
        let mut slot = self.0.stream.lock().await;
        self.check_open()?;
        let Some(stream) = slot.as_mut() else {
            return Err(StreamClosed::new_err("response closed"));
        };
        let next = tokio::select! {
            biased;
            _ = self.0.close_signal.notified() => {
                *slot = None;
                return Err(StreamClosed::new_err("response closed"));
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
        self.0.closed.store(
            true,
            Ordering::Release,
        );
        self.0.close_signal.notify_one();
    }

    /// Raise if the response was closed under the iterator; buffered pieces are
    /// not served after a close, only after the stream's own end.
    pub fn check_open(&self) -> Result<(), RqxError> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(StreamClosed::new_err("response closed"));
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
        let _ = self.0.decode_to_string(
            src, &mut out, last,
        );
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
            lines[0] = format!(
                "{}{}",
                self.buffer, lines[0]
            );
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
/// Without a size, chunks pass through as the network delivered them.
struct ByteChunker {
    size: Option<usize>,
    carry: BytesMut,
    head: Bytes,
}

impl ByteChunker {
    fn new(size: Option<usize>) -> Self {
        Self {
            size,
            carry: BytesMut::new(),
            head: Bytes::new(),
        }
    }

    fn feed(&mut self, mut bytes: Bytes) {
        if let Some(size) = self.size
            && !self.carry.is_empty()
        {
            let take = (size - self.carry.len()).min(bytes.len());
            self.carry.extend_from_slice(&bytes.split_to(take));
        }
        self.head = bytes;
    }

    /// The next full piece, if one is buffered.
    fn next_full(&mut self) -> Option<Bytes> {
        let Some(size) = self.size else {
            return (!self.head.is_empty()).then(|| std::mem::take(&mut self.head));
        };
        if self.carry.len() >= size {
            return Some(self.carry.split_to(size).freeze());
        }
        if self.head.len() >= size && self.carry.is_empty() {
            return Some(self.head.split_to(size));
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
/// Without a size, text passes through as each network chunk decodes.
struct TextChunker {
    size: Option<usize>,
    pending: String,
    consumed: usize,
}

impl TextChunker {
    fn new(size: Option<usize>) -> Self {
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
        let Some(size) = self.size else {
            self.consumed = self.pending.len();
            return (!unread.is_empty()).then(|| unread.to_string());
        };
        let (last_start, last) = unread.char_indices().nth(size - 1)?;
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

    pub fn close_blocking(self) {
        if let Body::Streaming(stream) = self {
            stream.close_blocking();
        }
    }

    pub fn is_closed(&self) -> bool {
        match self {
            Body::Streaming(stream) => stream.is_closed(),
            Body::Live(_) | Body::Buffered(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteChunker, LineDecoder, TextChunker};
    use bytes::Bytes;

    /// Feed `chunks` through a chunker of `size` and collect what it yields.
    fn rechunk(size: usize, chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut chunker = ByteChunker::new(Some(size));
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
                let pieces = rechunk(
                    size, &chunks,
                );
                assert!(
                    pieces[..pieces.len() - 1].iter().all(|p| p.len() == size),
                    "size {size} cut {cut}"
                );
                assert!(!pieces.last().unwrap().is_empty() && pieces.last().unwrap().len() <= size);
                assert_eq!(
                    pieces.concat(),
                    body,
                    "size {size} cut {cut}"
                );
            }
        }
    }

    #[test]
    fn chunkers_without_a_size_pass_chunks_through() {
        let mut bytes = ByteChunker::new(None);
        bytes.feed(Bytes::from_static(b"abc"));
        assert_eq!(
            bytes.next_full().as_deref(),
            Some(&b"abc"[..])
        );
        assert_eq!(
            bytes.next_full(),
            None
        );
        assert_eq!(
            bytes.flush(),
            None
        );
        let mut text = TextChunker::new(None);
        text.feed("héllo");
        assert_eq!(
            text.next_full().as_deref(),
            Some("héllo")
        );
        assert_eq!(
            text.next_full(),
            None
        );
        assert_eq!(
            text.flush(),
            None
        );
    }

    #[test]
    fn byte_chunker_empty_body_yields_nothing() {
        assert!(
            rechunk(
                16,
                &[]
            )
            .is_empty()
        );
        assert!(
            rechunk(
                16,
                &[b""]
            )
            .is_empty()
        );
    }

    #[test]
    fn text_chunker_counts_characters() {
        let mut chunker = TextChunker::new(Some(2));
        chunker.feed("aé€🙂b");
        assert_eq!(
            chunker.next_full().as_deref(),
            Some("aé")
        );
        assert_eq!(
            chunker.next_full().as_deref(),
            Some("€🙂")
        );
        assert_eq!(
            chunker.next_full(),
            None
        );
        assert_eq!(
            chunker.flush().as_deref(),
            Some("b")
        );
        assert_eq!(
            chunker.flush(),
            None
        );
    }

    #[test]
    fn text_chunker_yields_a_piece_of_exactly_size_without_waiting_for_more() {
        // A live stream that sends exactly `size` characters and pauses must not stall.
        let mut chunker = TextChunker::new(Some(3));
        chunker.feed("a€🙂");
        assert_eq!(
            chunker.next_full().as_deref(),
            Some("a€🙂")
        );
        assert_eq!(
            chunker.next_full(),
            None
        );
        chunker.feed("bc");
        assert_eq!(
            chunker.next_full(),
            None
        );
        chunker.feed("d");
        assert_eq!(
            chunker.next_full().as_deref(),
            Some("bcd")
        );
        assert_eq!(
            chunker.flush(),
            None
        );
    }

    #[test]
    fn split_lines_matches_splitlines() {
        assert_eq!(
            LineDecoder::split_lines("a\nb"),
            ["a", "b"]
        );
        assert_eq!(
            LineDecoder::split_lines("a\n"),
            ["a"]
        ); // no trailing empty after a terminator
        assert_eq!(
            LineDecoder::split_lines("a\n\n"),
            ["a", ""]
        );
        assert!(LineDecoder::split_lines("").is_empty());
        assert_eq!(
            LineDecoder::split_lines("a\r\nb"),
            ["a", "b"]
        ); // CRLF is a single break
        assert_eq!(
            LineDecoder::split_lines("a\rb"),
            ["a", "b"]
        ); // lone CR is a break
    }

    #[test]
    fn feed_emits_complete_lines() {
        let mut d = LineDecoder::default();
        assert_eq!(
            d.feed("a\nb\nc\n"),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn feed_buffers_partial_line_across_chunks() {
        let mut d = LineDecoder::default();
        assert!(d.feed("ab").is_empty()); // unterminated — buffered, nothing yet
        assert_eq!(
            d.feed("cd\n"),
            ["abcd"]
        ); // completed by the next chunk
    }

    #[test]
    fn feed_reassembles_crlf_split_across_chunks() {
        // The case we can't force over a socket: "\r\n" straddles the boundary.
        // The trailing "\r" must be deferred, not emitted as a lone-CR line.
        let mut d = LineDecoder::default();
        assert!(d.feed("a\r").is_empty()); // trailing CR deferred
        assert_eq!(
            d.feed("\nb\n"),
            ["a", "b"]
        ); // no spurious empty line
    }

    #[test]
    fn feed_treats_lone_cr_as_terminator() {
        let mut d = LineDecoder::default();
        assert_eq!(
            d.feed("a\rb\r"),
            ["a"]
        ); // "b" deferred (its own trailing CR)
        assert_eq!(
            d.flush(),
            Some("b".to_string())
        );
    }

    #[test]
    fn flush_emits_final_unterminated_line() {
        let mut d = LineDecoder::default();
        assert!(d.feed("last line").is_empty());
        assert_eq!(
            d.flush(),
            Some("last line".to_string())
        );
        assert_eq!(
            d.flush(),
            None
        ); // nothing left
    }
}
