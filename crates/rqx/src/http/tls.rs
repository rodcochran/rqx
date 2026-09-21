use pyo3::Bound;
use pyo3::prelude::PyResult;
use pyo3::types::{PyAny, PyAnyMethods, PyBool, PyBytes, PyString, PyTuple, PyTypeMethods};
use reqwest::Identity;
use reqwest::tls::Certificate;

use crate::exceptions::*;

use rqx_core::http::tls::{TlsIdentity, VerifyConfig};

pub fn parse_verify_config_from_py(verify: &Bound<'_, PyAny>) -> PyResult<VerifyConfig> {
    if verify.is_instance_of::<PyBool>() {
        let enabled = verify.extract::<bool>().unwrap();
        VerifyConfig::from_bool(enabled)

    } else if verify.is_instance_of::<PyString>() {
        let path: String = verify
            .extract::<String>()
            .map_err(|e| RqxError::new_err(format!("failed to parse CA cert path: {e}")))?;
        VerifyConfig::from_path_str(path)
    } else {
        Err(RqxError::new_err(format!(
            "verify must be bool or str (CA cert path), got {}",
            verify.get_type().name()?,
        )))
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
    if cert.is_instance_of::<PyString>() {
        let path: String = cert
            .extract()
            .map_err(|e| RqxError::new_err(format!("failed to parse client cert path: {e}")))?;
        TlsIdentity::from_path_str(path)?
    } else if cert.is_instance_of::<PyBytes>() {
        let bytes= cert.extract()
            .map_err(|e| RqxError::new_err(format!("failed to read cert bytes: {e}")))?
        TlsIdentity::from_pem_bytes(path)?

    } else if cert.is_instance_of::<PyTuple>() {
        let (cert_path, key_path): (String, String) = cert
            .extract()
            .map_err(|e| RqxError::new_err(format!("failed to parse cert, key tuple: {e}")))?;
        TlsIdentity::from_tuple((cert_path, key_path))?

    } else {
        return Err(RqxError::new_err(format!(
            "cert must be str (path), bytes (PEM), or (cert_path, key_path) tuple, got {}",
            cert.get_type().name()?,
        )));
    }
}
