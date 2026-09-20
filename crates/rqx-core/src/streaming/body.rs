
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