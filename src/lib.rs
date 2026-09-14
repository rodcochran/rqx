#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::redundant_field_names,
    clippy::needless_return
)]

use pyo3::prelude::*;
#[cfg(target_os = "macos")]
use pyo3::types::IntoPyDict;

mod client;
pub mod exceptions;
mod headers;
mod http;
mod py_json;
mod query_params;
mod request;
mod request_headers;
mod response;
mod retry;
mod runtime;
mod stream;
mod stream_context;
mod timeout;
mod transport;
mod url;

use client::{PyAsyncClient, PyClient};
use exceptions::*;
use headers::PyHeaders;
use response::PyResponse;
use retry::PyRetry;
use runtime::RUNTIME;
use stream::{PyAsyncStreamResponse, PyStreamResponse};
use stream_context::{PyAsyncStreamContext, PyStreamContext};
use timeout::PyTimeout;
use transport::{AsyncHTTPTransport, HTTPTransport};

/// `atexit` hook: shut the tokio runtime down before the interpreter starts
/// finalizing, so no tokio thread tries to attach to Python after that point
/// (https://github.com/rodcochran/rqx/issues/99). Runs with the GIL released because in-flight result deliveries may
/// need it to finish. See `runtime.rs` for the lifecycle as a whole.
#[pyfunction]
fn _shutdown_runtime(py: Python<'_>) {
    py.detach(|| RUNTIME.shutdown());
}

/// `os.register_at_fork(before=...)` hook: initialize the Apple frameworks a
/// client build touches while still in the parent, so the forked child does
/// not trip the Objective-C fork guard (https://github.com/rodcochran/rqx/issues/159). See `Runtime::prepare_fork`.
#[cfg(target_os = "macos")]
#[pyfunction]
fn _prepare_fork(py: Python<'_>) {
    py.detach(|| RUNTIME.prepare_fork());
}

#[pymodule]
fn _rqx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // The tokio runtime is deliberately NOT built here. It is created on first
    // use so that a process which only imports rqx (a prefork server's master)
    // never owns runtime threads to lose across fork() (https://github.com/rodcochran/rqx/issues/159).
    let py = m.py();
    m.add_function(wrap_pyfunction!(_shutdown_runtime, m)?)?;
    py.import("atexit")?
        .call_method1("register", (m.getattr("_shutdown_runtime")?,))?;
    #[cfg(target_os = "macos")]
    {
        m.add_function(wrap_pyfunction!(_prepare_fork, m)?)?;
        let kwargs = [("before", m.getattr("_prepare_fork")?)].into_py_dict(py)?;
        py.import("os")?
            .call_method("register_at_fork", (), Some(&kwargs))?;
    }
    m.add_class::<PyClient>()?;
    m.add_class::<PyAsyncClient>()?;
    m.add_class::<PyRetry>()?;
    m.add_class::<HTTPTransport>()?;
    m.add_class::<AsyncHTTPTransport>()?;
    m.add_class::<PyHeaders>()?;
    m.add_class::<PyTimeout>()?;
    m.add_class::<PyResponse>()?;
    m.add_class::<PyStreamResponse>()?;
    m.add_class::<PyAsyncStreamResponse>()?;
    m.add_class::<PyStreamContext>()?;
    m.add_class::<PyAsyncStreamContext>()?;
    m.add("RqxError", m.py().get_type::<RqxError>())?;
    m.add("HTTPError", m.py().get_type::<HTTPError>())?;
    m.add("RequestError", m.py().get_type::<RequestError>())?;
    m.add(
        "MaxRetriesExceeded",
        m.py().get_type::<MaxRetriesExceeded>(),
    )?;
    m.add("TransportError", m.py().get_type::<TransportError>())?;
    m.add("HTTPStatusError", m.py().get_type::<HTTPStatusError>())?;
    m.add("TimeoutException", m.py().get_type::<TimeoutException>())?;
    m.add("NetworkError", m.py().get_type::<NetworkError>())?;
    m.add("TooManyRedirects", m.py().get_type::<TooManyRedirects>())?;
    m.add("ProxyError", m.py().get_type::<ProxyError>())?;
    m.add("ProtocolError", m.py().get_type::<ProtocolError>())?;
    m.add(
        "RemoteProtocolError",
        m.py().get_type::<RemoteProtocolError>(),
    )?;
    m.add(
        "UnsupportedProtocol",
        m.py().get_type::<UnsupportedProtocol>(),
    )?;
    m.add("DecodingError", m.py().get_type::<DecodingError>())?;
    m.add("ConnectTimeout", m.py().get_type::<ConnectTimeout>())?;
    m.add("ReadTimeout", m.py().get_type::<ReadTimeout>())?;
    m.add("WriteTimeout", m.py().get_type::<WriteTimeout>())?;
    m.add("PoolTimeout", m.py().get_type::<PoolTimeout>())?;
    m.add("ConnectError", m.py().get_type::<ConnectError>())?;
    m.add("ReadError", m.py().get_type::<ReadError>())?;
    m.add("WriteError", m.py().get_type::<WriteError>())?;
    StdlibBackedExceptions::register(m)?;
    Ok(())
}
