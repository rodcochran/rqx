use pyo3::Bound;
use pyo3::types::{PyAny, PyAnyMethods, PyBool, PyBytes, PyString, PyTuple, PyTypeMethods};
use reqwest::Identity;

use rqx_core::http::tls::{IdentityParser, VerifyConfig};

use crate::exceptions::{PyRqxError, RqxError};

pub fn parse_verify_config_from_py(verify: &Bound<'_, PyAny>) -> Result<VerifyConfig, PyRqxError> {
    if verify.is_instance_of::<PyBool>() {
        let enabled = verify.extract::<bool>()?;
        Ok(VerifyConfig::from_bool(enabled))
    } else if verify.is_instance_of::<PyString>() {
        let path: String = verify.extract::<String>()?;
        Ok(VerifyConfig::from_path_str(path)?)
    } else {
        Err(RqxError::new_err(format!(
            "verify must be bool or str (CA cert path), got {}",
            verify.get_type().name()?,
        ))
        .into())
    }
}

/// Parses the Python `cert` argument into a reqwest `Identity`.
///
/// Accepts:
///   - `str` — path to a PEM file containing cert + key
///   - `bytes` — PEM bytes
///   - `(cert_path, key_path)` tuple — separate cert and key files (concatenated)
pub fn parse_identity(cert: &Bound<'_, PyAny>) -> Result<Identity, PyRqxError> {
    if cert.is_instance_of::<PyString>() {
        let path: String = cert.extract()?;
        Ok(IdentityParser::from_path_str(path)?)
    } else if cert.is_instance_of::<PyBytes>() {
        let bytes: Vec<u8> = cert.extract()?;
        Ok(IdentityParser::from_pem_bytes(&bytes)?)
    } else if cert.is_instance_of::<PyTuple>() {
        let (cert_path, key_path): (String, String) = cert.extract()?;
        Ok(IdentityParser::from_tuple((cert_path, key_path))?)
    } else {
        Err(RqxError::new_err(format!(
            "cert must be str (path), bytes (PEM), or (cert_path, key_path) tuple, got {}",
            cert.get_type().name()?,
        ))
        .into())
    }
}
