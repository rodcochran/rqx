use pyo3::prelude::*;

mod client;
mod runtime;
use client::PyClient;
use runtime::RUNTIME;

#[pymodule]
fn _reqx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyClient>()?;
    Ok(())
}
