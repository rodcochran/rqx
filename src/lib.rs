use pyo3::prelude::*;
use pyo3::exceptions::{PyRuntimeError};
use tokio::runtime::Builder as RtBuilder;

mod client;
mod py_json;
mod request;
mod response;
mod retry;
mod runtime;
mod transport;
mod stream;
pub mod exceptions;

use client::{PyClient, PyAsyncClient};
use runtime::RUNTIME;
use exceptions::*;
use retry::PyRetry;
use transport::{HTTPTransport, AsyncHTTPTransport};


#[pymodule]
fn _reqx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Multi-threaded tokio runtime (default worker count = num_cpus). H3 tried
    // worker_threads(1) but regressed throughput at c>=500 by ~20%; see
    // docs/improvements.md for the H3 experiment outcome.
    RUNTIME.set(
        RtBuilder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Error initializing Tokio Runtime: {e}")))
            ?
    ).expect("Runtime already initialized");
    // Share our runtime with pyo3-async-runtimes so the async path doesn't
    // silently build its own second default runtime alongside ours. We ignore
    // the Err case — it only fires if another imported PyO3 package already
    // initialized pyo3-async-runtimes' global runtime, in which case we share
    // that one instead. Still correct, just not the extra-threads win.
    let _ = pyo3_async_runtimes::tokio::init_with_runtime(
        RUNTIME.get().expect("RUNTIME just set"),
    );
    m.add_class::<PyClient>()?;
    m.add_class::<PyAsyncClient>()?;
    m.add_class::<PyRetry>()?;
    m.add_class::<HTTPTransport>()?;
    m.add_class::<AsyncHTTPTransport>()?;
    m.add("ReqxError", m.py().get_type::<ReqxError>())?;
    m.add("RequestError", m.py().get_type::<RequestError>())?;
    m.add("MaxRetriesExceeded", m.py().get_type::<MaxRetriesExceeded>())?;
    m.add("TransportError", m.py().get_type::<TransportError>())?;
    m.add("HTTPStatusError", m.py().get_type::<HTTPStatusError>())?;
    m.add("TimeoutException", m.py().get_type::<TimeoutException>())?;
    m.add("NetworkError", m.py().get_type::<NetworkError>())?;
    m.add("TooManyRedirects", m.py().get_type::<TooManyRedirects>())?;
    m.add("ProxyError", m.py().get_type::<ProxyError>())?;
    m.add("ConnectTimeout", m.py().get_type::<ConnectTimeout>())?;
    m.add("ReadTimeout", m.py().get_type::<ReadTimeout>())?;
    m.add("WriteTimeout", m.py().get_type::<WriteTimeout>())?;
    m.add("PoolTimeout", m.py().get_type::<PoolTimeout>())?;
    m.add("ConnectError", m.py().get_type::<ConnectError>())?;
    m.add("ReadError", m.py().get_type::<ReadError>())?;
    m.add("WriteError", m.py().get_type::<WriteError>())?;
    Ok(())
}
