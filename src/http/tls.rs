use pyo3::Bound;
use pyo3::prelude::PyResult;
use pyo3::types::{PyAny, PyAnyMethods, PyBool, PyBytes, PyString, PyTuple, PyTypeMethods};
use reqwest::Identity;
use reqwest::tls::Certificate;

use crate::exceptions::*;

/// Models the three meaningful states of the Python `verify` argument:
///   - `verify=True`  → use system root certificates (default TLS behavior).
///   - `verify=False` → accept invalid certificates (insecure).
///   - `verify="path"` → add a custom CA cert as a trusted root.
pub enum VerifyConfig {
    Default,
    DisableVerification,
    CustomCa(Certificate),
}

impl VerifyConfig {
    pub fn from_py_any(verify: &Bound<'_, PyAny>) -> PyResult<Self> {
        if verify.is_instance_of::<PyBool>() {
            let enabled = verify.extract::<bool>().unwrap();
            Ok(if enabled {
                Self::Default
            } else {
                Self::DisableVerification
            })
        } else if verify.is_instance_of::<PyString>() {
            let path = verify
                .extract::<String>()
                .map_err(|e| RqxError::new_err(format!("failed to parse CA cert path: {e}")))?;
            let bytes = std::fs::read(&path)
                .map_err(|e| RqxError::new_err(format!("failed to read CA cert: {e}")))?;
            let cert = Certificate::from_pem(&bytes)
                .map_err(|e| RqxError::new_err(format!("failed to construct CA cert: {e}")))?;
            Ok(Self::CustomCa(cert))
        } else {
            Err(RqxError::new_err(format!(
                "verify must be bool or str (CA cert path), got {}",
                verify.get_type().name()?,
            )))
        }
    }
}

/// Parses the Python `cert` argument into a reqwest `Identity`.
///
/// Accepts:
///   - `str` — path to a PEM file containing cert + key
///   - `bytes` — PEM bytes
///   - `(cert_path, key_path)` tuple — separate cert and key files (concatenated)
///
/// Each branch normalizes its input to a `Vec<u8>` of PEM bytes; the single
/// call to `Identity::from_pem` at the end handles construction and error
/// reporting uniformly.
pub fn parse_identity(cert: &Bound<'_, PyAny>) -> PyResult<Identity> {
    let pem_bytes: Vec<u8> = if cert.is_instance_of::<PyString>() {
        let path: String = cert
            .extract()
            .map_err(|e| RqxError::new_err(format!("failed to parse client cert path: {e}")))?;
        std::fs::read(&path)
            .map_err(|e| RqxError::new_err(format!("failed to read client cert: {e}")))?
    } else if cert.is_instance_of::<PyBytes>() {
        cert.extract()
            .map_err(|e| RqxError::new_err(format!("failed to read cert bytes: {e}")))?
    } else if cert.is_instance_of::<PyTuple>() {
        let (cert_path, key_path): (String, String) = cert
            .extract()
            .map_err(|e| RqxError::new_err(format!("failed to parse cert, key tuple: {e}")))?;
        let mut bytes = std::fs::read(&cert_path)
            .map_err(|e| RqxError::new_err(format!("failed to read {cert_path}: {e}")))?;
        let mut key_bytes = std::fs::read(&key_path)
            .map_err(|e| RqxError::new_err(format!("failed to read {key_path}: {e}")))?;
        bytes.append(&mut key_bytes);
        bytes
    } else {
        return Err(RqxError::new_err(format!(
            "cert must be str (path), bytes (PEM), or (cert_path, key_path) tuple, got {}",
            cert.get_type().name()?,
        )));
    };

    Identity::from_pem(&pem_bytes)
        .map_err(|e| RqxError::new_err(format!("failed to construct client cert: {e}")))
}
