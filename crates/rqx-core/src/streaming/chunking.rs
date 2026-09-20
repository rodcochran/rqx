use bytes::{Bytes, BytesMut};

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
