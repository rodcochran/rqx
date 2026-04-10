use std::collections::HashMap;
use http::{StatusCode};
use pyo3::prelude::{Py, PyAny, PyResult, Python, pyclass, pymethods};

use super::exceptions::{ReqxError, HTTPStatusError};
use super::py_json::{value_to_py};


#[pyclass]
pub struct PyResponse {
    #[pyo3(get)]
    pub status_code: u16,

    #[pyo3(get)]
    pub headers: HashMap<String, String>,

    #[pyo3(get)]
    pub content: Vec<u8>,
    
    #[pyo3(get)]
    pub url: String,

    #[pyo3(get)]
    pub(crate) elapsed: f64
}

#[pymethods]
impl PyResponse {
    fn text(&self) -> String {
        // might want to revisit this... particularly the unwrap()
        std::str::from_utf8(&self.content).unwrap().to_string()
    }

    fn json(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        serde_json::from_str(&self.text())
            .map_err(|e| ReqxError::new_err(format!("invalid JSON response: {e}")))
            .and_then(|v| value_to_py(py, v))
    }

    fn raise_for_status(&self) -> PyResult<()> {
        let s_result = StatusCode::from_u16(self.status_code);
        match s_result {
            Ok(s) => {
                if !s.is_success() {
                    Err(HTTPStatusError::new_err(format!("{} error", self.status_code)))
                }
                else {
                    Ok(())
                }
            }
            Err(e) => {
                Err(ReqxError::new_err(format!("invalid Status Code: {e}")))
            }
        }
    }
}