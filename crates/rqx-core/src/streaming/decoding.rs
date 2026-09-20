use encoding_rs::{Decoder, Encoding};

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
