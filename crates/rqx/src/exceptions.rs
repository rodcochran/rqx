use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::{Bound, PyAny, PyAnyMethods, PyModule, PyModuleMethods, PyResult};
use pyo3::types::{PyDict, PyType};
use pyo3::{PyErr, create_exception, import_exception};

use rqx_core::error::RqxCoreError;

/*

Exception hierarchy, same shape as httpx so `except` clauses port unchanged.

rqx.RqxError
└── rqx.HTTPError                    (catch-all for anything a request can raise)
    ├── rqx.RequestError             (failed before a usable response arrived)
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
    │   │   ├── rqx.ProtocolError
    │   │   │   └── rqx.RemoteProtocolError   (the server broke HTTP)
    │   │   ├── rqx.ProxyError               (the proxy refused the CONNECT)
    │   │   └── rqx.UnsupportedProtocol      (URL scheme is not http/https)
    │   ├── rqx.DecodingError            (Content-Encoding the decoder rejected)
    │   └── rqx.TooManyRedirects
    ├── rqx.HTTPStatusError          (raised by raise_for_status(); carries .response)
    └── rqx.MaxRetriesExceeded       (raised when retries are exhausted)

Also under rqx.RqxError, each with a stdlib base as well (built in `StdlibBackedExceptions`):

rqx.InvalidURL                       (a URL that can't be parsed; also a ValueError)
rqx.JSONDecodeError                  (response.json(); also a json.JSONDecodeError)
rqx.StreamError                      (misusing a stream; also a RuntimeError)
├── rqx.StreamConsumed               (read or iterated twice)
├── rqx.StreamClosed                 (used after close)
└── rqx.ResponseNotRead              (.content / .text / .json() before read)
*/

// Level 1
create_exception!(rqx, RqxError, pyo3::exceptions::PyException);

// Level 2
create_exception!(rqx, HTTPError, RqxError);

// Level 3
create_exception!(rqx, RequestError, HTTPError);
create_exception!(rqx, HTTPStatusError, HTTPError);
create_exception!(rqx, MaxRetriesExceeded, HTTPError);

// Level 4
create_exception!(rqx, TransportError, RequestError);
create_exception!(rqx, DecodingError, RequestError);
create_exception!(rqx, TooManyRedirects, RequestError);

// Level 5
create_exception!(rqx, TimeoutException, TransportError);
create_exception!(rqx, NetworkError, TransportError);
create_exception!(rqx, ProtocolError, TransportError);
create_exception!(rqx, ProxyError, TransportError);
create_exception!(rqx, UnsupportedProtocol, TransportError);

// Level 6
create_exception!(rqx, RemoteProtocolError, ProtocolError);
create_exception!(rqx, ConnectTimeout, TimeoutException);
create_exception!(rqx, ReadTimeout, TimeoutException);
create_exception!(rqx, WriteTimeout, TimeoutException);
create_exception!(rqx, PoolTimeout, TimeoutException);
create_exception!(rqx, ConnectError, NetworkError);
create_exception!(rqx, ReadError, NetworkError);
create_exception!(rqx, WriteError, NetworkError);

// Classes with a stdlib base as well as an rqx one. `create_exception!` takes a single
// base, so these are built with `type()` when the module loads and raised through
// `import_exception!`, which looks them up on `rqx._rqx` the first time one is raised.
import_exception!(rqx._rqx, InvalidURL);
import_exception!(rqx._rqx, JSONDecodeError);
import_exception!(rqx._rqx, StreamError);
import_exception!(rqx._rqx, StreamConsumed);
import_exception!(rqx._rqx, StreamClosed);
import_exception!(rqx._rqx, ResponseNotRead);

pub struct StdlibBackedExceptions;

impl StdlibBackedExceptions {
    pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
        let py = m.py();
        let rqx_error = py.get_type::<RqxError>().into_any();
        let stdlib_json_error = py.import("json")?.getattr("JSONDecodeError")?;
        let value_error = py.get_type::<PyValueError>().into_any();
        let runtime_error = py.get_type::<PyRuntimeError>().into_any();

        Self::define(
            m,
            "InvalidURL",
            (rqx_error.clone(), value_error),
            "A URL that couldn't be parsed. Also a ValueError.",
        )?;
        Self::define(
            m,
            "JSONDecodeError",
            (rqx_error.clone(), stdlib_json_error),
            "response.json() found a body that isn't JSON. Also a json.JSONDecodeError.",
        )?;
        let stream_error = Self::define(
            m,
            "StreamError",
            (rqx_error, runtime_error),
            "A streamed response was used in a way it can't be. Also a RuntimeError.",
        )?;
        for (name, doc) in [
            (
                "StreamConsumed",
                "The body was already read or iterated; it can be consumed once.",
            ),
            (
                "StreamClosed",
                "The response was closed before the body was read.",
            ),
            (
                "ResponseNotRead",
                "The body hasn't been read; call read() or aread() first.",
            ),
        ] {
            Self::define(m, name, (stream_error.clone(),), doc)?;
        }
        Ok(())
    }

    fn define<'py>(
        m: &Bound<'py, PyModule>,
        name: &str,
        bases: impl pyo3::IntoPyObject<'py>,
        doc: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let py = m.py();
        let namespace = PyDict::new(py);
        namespace.set_item("__module__", "rqx")?;
        namespace.set_item("__doc__", doc)?;
        let class = py.get_type::<PyType>().call1((name, bases, namespace))?;
        m.add(name, &class)?;
        Ok(class)
    }
}

/// The error a pymethod returns: `?` works on both core results and pyo3
/// results, and pyo3 converts it to the matching Python exception on the way out.
pub enum PyRqxError {
    Core(RqxCoreError),
    Py(PyErr),
}

impl From<RqxCoreError> for PyRqxError {
    fn from(value: RqxCoreError) -> Self {
        PyRqxError::Core(value)
    }
}

impl From<PyErr> for PyRqxError {
    fn from(value: PyErr) -> Self {
        PyRqxError::Py(value)
    }
}

impl From<PyRqxError> for PyErr {
    fn from(value: PyRqxError) -> Self {
        match value {
            PyRqxError::Py(e) => e,
            PyRqxError::Core(e) => PyRqxError::core(e),
        }
    }
}

impl PyRqxError {
    fn core(e: RqxCoreError) -> PyErr {
        match e {
            RqxCoreError::ConnectTimeout(message) => ConnectTimeout::new_err(message),
            RqxCoreError::ReadTimeout(message) => ReadTimeout::new_err(message),
            RqxCoreError::ConnectError(message) => ConnectError::new_err(message),
            RqxCoreError::ReadError(message) => ReadError::new_err(message),
            RqxCoreError::RemoteProtocolError(message) => RemoteProtocolError::new_err(message),
            RqxCoreError::ProxyError(message) => ProxyError::new_err(message),
            RqxCoreError::UnsupportedProtocol(message) => UnsupportedProtocol::new_err(message),
            RqxCoreError::DecodingError(message) => DecodingError::new_err(message),
            RqxCoreError::TooManyRedirects(message) => TooManyRedirects::new_err(message),
            RqxCoreError::RequestError(message) => RequestError::new_err(message),
            RqxCoreError::HTTPStatusError(message) => HTTPStatusError::new_err(message),
            RqxCoreError::MaxRetriesExceeded(message) => MaxRetriesExceeded::new_err(message),
            RqxCoreError::InvalidURL(message) => InvalidURL::new_err(message),
            RqxCoreError::JSONDecodeError(e) => JSONDecodeError::new_err((e.message, e.doc, e.pos)),
            RqxCoreError::StreamClosed(message) => StreamClosed::new_err(message),
            RqxCoreError::StreamError(message) => StreamError::new_err(message),
            RqxCoreError::TLSConfigError(message) => RqxError::new_err(message),
            RqxCoreError::InvalidArgument(message) => PyValueError::new_err(message),
            RqxCoreError::UnknownKeyword(message) => PyTypeError::new_err(message),
            RqxCoreError::MissingKey(key) => PyKeyError::new_err(key),
        }
    }
}
