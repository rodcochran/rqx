use std::error::Error;

#[derive(Debug)]
pub enum RqxError {
    HTTPError(HTTPError),
    InvalidURL(String),
    JSONDecodeError(String),
    StreamError(StreamError),
}

impl Error for RqxError {}

impl std::fmt::Display for RqxError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RqxError::HTTPError(e) => write!(
                f,
                "{e}"
            ),
            RqxError::InvalidURL(e) => write!(
                f,
                "{e}"
            ),
            RqxError::JSONDecodeError(e) => write!(
                f,
                "{e}"
            ),
            RqxError::StreamError(e) => write!(
                f,
                "{e}"
            ),
        }
    }
}

#[derive(Debug)]
pub enum HTTPError {
    RequestError(RequestError),
    HTTPStatusError(String),
    MaxRetriesExceeded(String),
}

impl Error for HTTPError {}

impl From<HTTPError> for RqxError {
    fn from(value: HTTPError) -> Self {
        RqxError::HTTPError(value)
    }
}

impl std::fmt::Display for HTTPError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            HTTPError::RequestError(e) => write!(
                f,
                "{e}"
            ),
            HTTPError::HTTPStatusError(e) => write!(
                f,
                "{e}"
            ),
            HTTPError::MaxRetriesExceeded(e) => write!(
                f,
                "{e}"
            ),
        }
    }
}

#[derive(Debug)]
pub enum RequestError {
    TransportError(TransportError),
    DecodingError(String),
    TooManyRedirects(String),
    RequestError(String),
}

impl Error for RequestError {}

impl From<RequestError> for RqxError {
    fn from(value: RequestError) -> Self {
        RqxError::from(HTTPError::RequestError(value))
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RequestError::TransportError(e) => write!(
                f,
                "{e}"
            ),
            RequestError::DecodingError(e) => write!(
                f,
                "{e}"
            ),
            RequestError::TooManyRedirects(e) => write!(
                f,
                "{e}"
            ),
            RequestError::RequestError(e) => write!(
                f,
                "{e}"
            ),
        }
    }
}

#[derive(Debug)]
pub enum TransportError {
    TimeoutException(TimeoutException),
    NetworkError(NetworkError),
    ProtocolError(ProtocolError),
    ProxyError(String),
    UnsupportedProtocol(String),
}

impl Error for TransportError {}

impl From<TransportError> for RqxError {
    fn from(value: TransportError) -> Self {
        RqxError::from(RequestError::TransportError(value))
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TransportError::TimeoutException(e) => write!(
                f,
                "{e}"
            ),
            TransportError::NetworkError(e) => write!(
                f,
                "{e}"
            ),
            TransportError::ProtocolError(e) => write!(
                f,
                "{e}"
            ),
            TransportError::ProxyError(e) => write!(
                f,
                "{e}"
            ),
            TransportError::UnsupportedProtocol(e) => write!(
                f,
                "{e}"
            ),
        }
    }
}

#[derive(Debug)]
pub enum TimeoutException {
    ConnectTimeout(String),
    ReadTimeout(String),
    WriteTimeout(String),
    PoolTimeout(String),
}

impl Error for TimeoutException {}

impl From<TimeoutException> for RqxError {
    fn from(value: TimeoutException) -> Self {
        RqxError::from(TransportError::TimeoutException(value))
    }
}

impl std::fmt::Display for TimeoutException {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TimeoutException::ConnectTimeout(e) => write!(
                f,
                "{e}"
            ),
            TimeoutException::ReadTimeout(e) => write!(
                f,
                "{e}"
            ),
            TimeoutException::WriteTimeout(e) => write!(
                f,
                "{e}"
            ),
            TimeoutException::PoolTimeout(e) => write!(
                f,
                "{e}"
            ),
        }
    }
}

#[derive(Debug)]
pub enum NetworkError {
    ConnectError(String),
    ReadError(String),
    WriteError(String),
}

impl Error for NetworkError {}

impl From<NetworkError> for RqxError {
    fn from(value: NetworkError) -> Self {
        RqxError::from(TransportError::NetworkError(value))
    }
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NetworkError::ConnectError(e) => write!(
                f,
                "{e}"
            ),
            NetworkError::ReadError(e) => write!(
                f,
                "{e}"
            ),
            NetworkError::WriteError(e) => write!(
                f,
                "{e}"
            ),
        }
    }
}

#[derive(Debug)]
pub enum ProtocolError {
    RemoteProtocolError(String),
}

impl Error for ProtocolError {}

impl From<ProtocolError> for RqxError {
    fn from(value: ProtocolError) -> Self {
        RqxError::from(TransportError::ProtocolError(value))
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ProtocolError::RemoteProtocolError(e) => write!(
                f,
                "{e}"
            ),
        }
    }
}

#[derive(Debug)]
pub enum StreamError {
    StreamConsumed(String),
    StreamClosed(String),
    ResponseNotRead(String),
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            StreamError::StreamConsumed(e) => write!(
                f,
                "{e}"
            ),
            StreamError::StreamClosed(e) => write!(
                f,
                "{e}"
            ),
            StreamError::ResponseNotRead(e) => write!(
                f,
                "{e}"
            ),
        }
    }
}

// hyper-util keeps its tunnel error type private, so its text is all there is to match.
const TUNNEL_REFUSED: [&str; 2] = [
    "tunnel error: unsuccessful",
    "tunnel error: proxy authorization required",
];

impl From<reqwest::Error> for RqxError {
    fn from(value: reqwest::Error) -> Self {
        let msg = format!("{value}");
        let sources = || {
            std::iter::successors(
                value.source(),
                |s| (*s).source(),
            )
        };

        if value.is_timeout() {
            // Timeout — disambiguate connect-phase vs read-phase. Write timeouts
            // are rare enough that we don't try to detect them; they'll surface
            // as ReadTimeout, which is acceptable for v0.
            if value.is_connect() {
                return TimeoutException::ConnectTimeout(msg).into();
            }
            return TimeoutException::ReadTimeout(msg).into();
        }

        if value.is_connect() {
            if sources().any(|s| TUNNEL_REFUSED.contains(&s.to_string().as_str())) {
                return TransportError::ProxyError(msg).into();
            }
            return NetworkError::ConnectError(msg).into();
        }

        if value.is_redirect() {
            return RequestError::TooManyRedirects(msg).into();
        }

        if let Some(hyper) = sources().find_map(|s| s.downcast_ref::<hyper::Error>()) {
            // The server broke HTTP, unless the network failed underneath hyper.
            if hyper.is_parse() || hyper.is_incomplete_message() {
                return ProtocolError::RemoteProtocolError(msg).into();
            }
            let os_error = hyper
                .source()
                .and_then(|s| s.downcast_ref::<std::io::Error>())
                .and_then(|io| io.raw_os_error());
            return match os_error {
                Some(_) => NetworkError::ReadError(msg).into(),
                None => ProtocolError::RemoteProtocolError(msg).into(),
            };
        }

        if value.is_body() || value.is_decode() {
            // No transport error underneath: the decompressor rejected the body.
            return RequestError::DecodingError(msg).into();
        }

        // Anything else still failed before a response arrived, so it stays
        // under RequestError and `except HTTPError` catches it.
        RequestError::RequestError(format!("request failed: {value}")).into()
    }
}
