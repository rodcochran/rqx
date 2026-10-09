use http::header::{InvalidHeaderName, InvalidHeaderValue, MaxSizeReached};
use std::error::Error;

/// Every error core produces. Each variant is raised as one Python exception class,
/// named after it unless noted; the mapping and class tree are in `rqx/src/exceptions.rs`.
#[derive(Debug)]
pub enum RqxCoreError {
    ConnectTimeout(String),
    ReadTimeout(String),
    ConnectError(String),
    ReadError(String),
    RemoteProtocolError(String),
    ProxyError(String),
    UnsupportedProtocol(String),
    DecodingError(String),
    TooManyRedirects(String),
    RequestError(String),
    HTTPStatusError(String),
    MaxRetriesExceeded(String),
    InvalidURL(String),
    JSONDecodeError(JSONDecodeError),
    StreamClosed(String),
    StreamError(String),
    /// Raised as the base `rqx.RqxError`.
    TLSConfigError(String),
    /// Raised as `ValueError`.
    InvalidArgument(String),
    /// Raised as `TypeError`.
    UnknownKeyword(String),
    /// Raised as `KeyError`.
    MissingKey(String),
}

impl Error for RqxCoreError {}

impl std::fmt::Display for RqxCoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RqxCoreError::JSONDecodeError(e) => write!(f, "{}", e.message),
            RqxCoreError::ConnectTimeout(message)
            | RqxCoreError::ReadTimeout(message)
            | RqxCoreError::ConnectError(message)
            | RqxCoreError::ReadError(message)
            | RqxCoreError::RemoteProtocolError(message)
            | RqxCoreError::ProxyError(message)
            | RqxCoreError::UnsupportedProtocol(message)
            | RqxCoreError::DecodingError(message)
            | RqxCoreError::TooManyRedirects(message)
            | RqxCoreError::RequestError(message)
            | RqxCoreError::HTTPStatusError(message)
            | RqxCoreError::MaxRetriesExceeded(message)
            | RqxCoreError::InvalidURL(message)
            | RqxCoreError::StreamClosed(message)
            | RqxCoreError::StreamError(message)
            | RqxCoreError::TLSConfigError(message)
            | RqxCoreError::InvalidArgument(message)
            | RqxCoreError::UnknownKeyword(message)
            | RqxCoreError::MissingKey(message) => write!(f, "{message}"),
        }
    }
}

#[derive(Debug)]
pub struct JSONDecodeError {
    pub message: String,
    pub doc: String,
    pub pos: usize,
}

// hyper-util keeps its tunnel error type private, so its text is all there is to match.
const TUNNEL_REFUSED: [&str; 2] = [
    "tunnel error: unsuccessful",
    "tunnel error: proxy authorization required",
];

impl From<reqwest::Error> for RqxCoreError {
    fn from(value: reqwest::Error) -> Self {
        let msg = format!("{value}");
        let sources = || std::iter::successors(value.source(), |s| (*s).source());

        if value.is_timeout() {
            // Timeout — disambiguate connect-phase vs read-phase. Write timeouts
            // are rare enough that we don't try to detect them; they'll surface
            // as ReadTimeout, which is acceptable for v0.
            if value.is_connect() {
                return RqxCoreError::ConnectTimeout(msg);
            }
            return RqxCoreError::ReadTimeout(msg);
        }

        if value.is_connect() {
            if sources().any(|s| TUNNEL_REFUSED.contains(&s.to_string().as_str())) {
                return RqxCoreError::ProxyError(msg);
            }
            return RqxCoreError::ConnectError(msg);
        }

        if value.is_redirect() {
            return RqxCoreError::TooManyRedirects(msg);
        }

        if let Some(hyper) = sources().find_map(|s| s.downcast_ref::<hyper::Error>()) {
            // The server broke HTTP, unless the network failed underneath hyper.
            if hyper.is_parse() || hyper.is_incomplete_message() {
                return RqxCoreError::RemoteProtocolError(msg);
            }
            let os_error = hyper
                .source()
                .and_then(|s| s.downcast_ref::<std::io::Error>())
                .and_then(|io| io.raw_os_error());
            return match os_error {
                Some(_) => RqxCoreError::ReadError(msg),
                None => RqxCoreError::RemoteProtocolError(msg),
            };
        }

        if value.is_body() || value.is_decode() {
            // No transport error underneath: the decompressor rejected the body.
            return RqxCoreError::DecodingError(msg);
        }

        // Anything else still failed before a response arrived, so it stays
        // under RequestError and `except HTTPError` catches it.
        RqxCoreError::RequestError(format!("request failed: {value}"))
    }
}

impl From<InvalidHeaderName> for RqxCoreError {
    fn from(value: InvalidHeaderName) -> Self {
        RqxCoreError::InvalidArgument(format!("invalid header name {value}"))
    }
}

impl From<InvalidHeaderValue> for RqxCoreError {
    fn from(value: InvalidHeaderValue) -> Self {
        RqxCoreError::InvalidArgument(format!("invalid header value {value}"))
    }
}

impl From<MaxSizeReached> for RqxCoreError {
    fn from(value: MaxSizeReached) -> Self {
        RqxCoreError::InvalidArgument(format!("too many headers: {value}"))
    }
}
