use pyo3::prelude::*;
use pyo3::exceptions::{PyRuntimeError};
use tokio::runtime::Runtime;

mod client;
mod py_json;
mod request;
mod response;
mod retry;
mod runtime;
pub mod exceptions;
use client::{PyClient, PyAsyncClient};
use runtime::RUNTIME;
use exceptions::*;

#[pymodule]
fn _reqx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    RUNTIME.set(
        Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Error initializing Tokio Runtime: {e}")))
            ?
    ).expect("Runtime already initialized");
    m.add_class::<PyClient>()?;
    m.add_class::<PyAsyncClient>()?;
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
