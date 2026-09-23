use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use futures::{Stream, StreamExt};

use tokio::sync::Mutex as TokioMutex;
use tokio::sync::Notify;

use crate::error::{RqxError, StreamError};

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
        Self(Arc::new(LiveStreamInner {
            stream: TokioMutex::new(Some(Box::pin(response.bytes_stream()))),
            closed: AtomicBool::new(false),
            close_signal: Notify::new(),
        }))
    }

    /// The next chunk, `None` at the end. A closed response or a failed read
    /// drops the stream, so the connection is released without waiting for
    /// the iterator to be dropped.
    pub async fn next_chunk(&self) -> Result<Option<Bytes>, RqxError> {
        let mut slot = self.0.stream.lock().await;
        self.check_open()?;
        let Some(stream) = slot.as_mut() else {
            return Err(StreamError::StreamClosed("response closed".to_string()).into());
        };
        let next = tokio::select! {
            biased;
            _ = self.0.close_signal.notified() => {
                *slot = None;
                return Err(StreamError::StreamClosed("response closed".to_string()).into());
            }
            next = stream.next() => next,
        };
        match next {
            Some(Ok(bytes)) => Ok(Some(bytes)),
            Some(Err(e)) => {
                *slot = None;
                Err(RqxError::from(e))
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
    pub(crate) async fn close(&self) {
        self.mark_closed();
        *self.0.stream.lock().await = None;
    }

    /// For the sync response, which closes from the Python thread, off the runtime.
    pub(crate) fn close_blocking(&self) {
        self.mark_closed();
        *self.0.stream.blocking_lock() = None;
    }

    pub(crate) fn mark_closed(&self) {
        self.0.closed.store(true, Ordering::Release);
        self.0.close_signal.notify_one();
    }

    /// Raise if the response was closed under the iterator; buffered pieces are
    /// not served after a close, only after the stream's own end.
    pub fn check_open(&self) -> Result<(), RqxError> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(StreamError::StreamClosed("response closed".to_string()).into());
        }
        Ok(())
    }

    /// Closed, or read to the end. A poll in flight holds the lock; that counts as open.
    pub fn is_closed(&self) -> bool {
        self.0.closed.load(Ordering::Acquire)
            || self
                .0
                .stream
                .try_lock()
                .map(|slot| slot.is_none())
                .unwrap_or(false)
    }
}
