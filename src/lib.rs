use pyo3::prelude::*;
use pyo3::exceptions::{PyRuntimeError};
use tokio::runtime::Runtime;


mod client;
mod runtime;
use client::PyClient;
use runtime::RUNTIME;

#[pymodule]
fn _reqx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    RUNTIME.set(
        Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Error initializing Tokio Runtime: {e}")))
            ?
    ).expect("Runtime already initialized");
    m.add_class::<PyClient>()?;
    Ok(())
}
