use http::header::{InvalidHeaderName, InvalidHeaderValue, MaxSizeReached};
use std::error::Error;

/// Every error core produces. Each variant is raised as one Python exception class,
/// named after it unless noted; the mapping and class tree are in `rqx/src/exceptions.rs`.
#[derive(Debug)]
pub enum RqxError {
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

impl Error for RqxError {}

impl std::fmt::Display for RqxError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RqxError::JSONDecodeError(e) => write!(f, "{}", e.message),
            RqxError::ConnectTimeout(m)
            | RqxError::ReadTimeout(m)
            | RqxError::ConnectError(m)
            | RqxError::ReadError(m)
            | RqxError::RemoteProtocolError(m)
            | RqxError::ProxyError(m)
            | RqxError::UnsupportedProtocol(m)
            | RqxError::DecodingError(m)
            | RqxError::TooManyRedirects(m)
            | RqxError::RequestError(m)
            | RqxError::HTTPStatusError(m)
            | RqxError::MaxRetriesExceeded(m)
            | RqxError::InvalidURL(m)
            | RqxError::StreamClosed(m)
            | RqxError::StreamError(m)
            | RqxError::TLSConfigError(m)
            | RqxError::InvalidArgument(m)
            | RqxError::UnknownKeyword(m)
            | RqxError::MissingKey(m) => write!(f, "{m}"),
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

impl From<reqwest::Error> for RqxError {
    fn from(value: reqwest::Error) -> Self {
        let msg = format!("{value}");
        let sources = || std::iter::successors(value.source(), |s| (*s).source());

        if value.is_timeout() {
            // Timeout — disambiguate connect-phase vs read-phase. Write timeouts
            // are rare enough that we don't try to detect them; they'll surface
            // as ReadTimeout, which is acceptable for v0.
            if value.is_connect() {
                return RqxError::ConnectTimeout(msg);
            }
            return RqxError::ReadTimeout(msg);
        }

        if value.is_connect() {
            if sources().any(|s| TUNNEL_REFUSED.contains(&s.to_string().as_str())) {
                return RqxError::ProxyError(msg);
            }
            return RqxError::ConnectError(msg);
        }

        if value.is_redirect() {
            return RqxError::TooManyRedirects(msg);
        }

        if let Some(hyper) = sources().find_map(|s| s.downcast_ref::<hyper::Error>()) {
            // The server broke HTTP, unless the network failed underneath hyper.
            if hyper.is_parse() || hyper.is_incomplete_message() {
                return RqxError::RemoteProtocolError(msg);
            }
            let os_error = hyper
                .source()
                .and_then(|s| s.downcast_ref::<std::io::Error>())
                .and_then(|io| io.raw_os_error());
            return match os_error {
                Some(_) => RqxError::ReadError(msg),
                None => RqxError::RemoteProtocolError(msg),
            };
        }

        if value.is_body() || value.is_decode() {
            // No transport error underneath: the decompressor rejected the body.
            return RqxError::DecodingError(msg);
        }

        // Anything else still failed before a response arrived, so it stays
        // under RequestError and `except HTTPError` catches it.
        RqxError::RequestError(format!("request failed: {value}"))
    }
}

impl From<InvalidHeaderName> for RqxError {
    fn from(value: InvalidHeaderName) -> Self {
        RqxError::InvalidArgument(format!("invalid header name {value}"))
    }
}

impl From<InvalidHeaderValue> for RqxError {
    fn from(value: InvalidHeaderValue) -> Self {
        RqxError::InvalidArgument(format!("invalid header value {value}"))
    }
}

impl From<MaxSizeReached> for RqxError {
    fn from(value: MaxSizeReached) -> Self {
        RqxError::InvalidArgument(format!("too many headers: {value}"))
    }
}
