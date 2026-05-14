use pyo3::PyErr;
use pyo3::create_exception;

/*

Exception Hierarchy to provide a drop-in replacement interface for Httpx

rqx.RqxError
├── rqx.RequestError
│   ├── rqx.TransportError
│   │   ├── rqx.TimeoutException
│   │   │   ├── rqx.ConnectTimeout
│   │   │   ├── rqx.ReadTimeout
│   │   │   ├── rqx.WriteTimeout
│   │   │   └── rqx.PoolTimeout
│   │   ├── rqx.NetworkError
│   │   │   ├── rqx.ConnectError
│   │   │   ├── rqx.ReadError
│   │   │   └── rqx.WriteError
│   │   ├── rqx.TooManyRedirects
│   │   └── rqx.ProxyError
│   └── rqx.HTTPStatusError          (raised by raise_for_status())
└── rqx.MaxRetriesExceeded           (raised when retry budget exhausted)
*/


// Level 1
create_exception!(rqx, RqxError, pyo3::exceptions::PyException);

// Level 2
create_exception!(rqx, RequestError, RqxError);
create_exception!(rqx, MaxRetriesExceeded, RqxError);

// Level 3
create_exception!(rqx, TransportError, RequestError);
create_exception!(rqx, HTTPStatusError, RequestError);
create_exception!(rqx, TooManyRedirects, RequestError);

// Level 4
create_exception!(rqx, TimeoutException, TransportError);
create_exception!(rqx, NetworkError, TransportError);
create_exception!(rqx, ProxyError, TransportError);

// Level 5
create_exception!(rqx, ConnectTimeout, TimeoutException);
create_exception!(rqx, ReadTimeout, TimeoutException);
create_exception!(rqx, WriteTimeout, TimeoutException);
create_exception!(rqx, PoolTimeout, TimeoutException);
create_exception!(rqx, ConnectError, NetworkError);
create_exception!(rqx, ReadError, NetworkError);
create_exception!(rqx, WriteError, NetworkError);


/// Map a reqwest error to the most specific rqx exception type.
///
/// reqwest exposes a few classification predicates (`is_timeout`, `is_connect`,
/// `is_body`, etc.) but doesn't fully disambiguate, e.g. between
/// `ConnectTimeout` and `ReadTimeout`. We use the predicates that exist and
/// fall back to the broader category for the rest. Leaf types all inherit
/// from `RqxError`, so callers that catch `RqxError` continue to work.
pub fn map_reqwest_error(e: reqwest::Error) -> PyErr {
    let msg = format!("{e}");

    if e.is_timeout() {
        // Timeout — disambiguate connect-phase vs read-phase. Write timeouts
        // are rare enough that we don't try to detect them; they'll surface
        // as ReadTimeout, which is acceptable for v0.
        if e.is_connect() {
            return ConnectTimeout::new_err(msg);
        }
        return ReadTimeout::new_err(msg);
    }

    if e.is_connect() {
        return ConnectError::new_err(msg);
    }

    if e.is_body() || e.is_decode() {
        return ReadError::new_err(msg);
    }

    if e.is_redirect() {
        return TooManyRedirects::new_err(msg);
    }

    // Fallback: keep callers' "request failed" prefix for grep-friendliness
    // with the pre-mapping error messages.
    RqxError::new_err(format!("request failed: {e}"))
}
