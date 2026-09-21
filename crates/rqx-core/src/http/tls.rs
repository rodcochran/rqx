use reqwest::Identity;
use reqwest::tls::Certificate;

use crate::error::*;

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
    pub fn from_bool(v: bool) -> Result<VerifyConfig, _> {
        Ok(if v {
            Self::Default
        } else {
            Self::DisableVerification
        })
    }

    pub fn from_path_str(path: String) -> Result<VerifyConfig, RqxError> {
        let bytes = std::fs::read(&path)
            .map_err(|e| RqxError::TLSConfigError(format!("failed to read CA cert: {e}")))?;

        let cert = Certificate::from_pem(&bytes)
            .map_err(|e| RqxError::TLSConfigError(format!("failed to construct CA cert: {e}")))?;
        Ok(Self::CustomCa(cert))
    }
}

pub struct IdentityParser {}

impl IdentityParser {
    pub fn from_path_str(path: String) -> Result<Identity, RqxError> {
        let pem_bytes = std::fs::read(&path)
            .map_err(|e| RqxError::TLSConfigError(format!("failed to read client cert: {e}")))?;

        Self::from_pem_bytes(&pem_bytes)
    }

    pub fn from_tuple(cert: (String, String)) -> Result<Identity, RqxError> {
        let (cert_path, key_path) = cert;
        let mut bytes = std::fs::read(&cert_path)
            .map_err(|e| RqxError::TLSConfigError(format!("failed to read {cert_path}: {e}")))?;
        let mut key_bytes = std::fs::read(&key_path)
            .map_err(|e| RqxError::TLSConfigError(format!("failed to read {key_path}: {e}")))?;
        bytes.append(&mut key_bytes);

        Self::from_pem_bytes(&bytes)
    }

    pub fn from_pem_bytes(pem_bytes: &[u8]) -> Result<Identity, RqxError> {
        Ok(Identity::from_pem(&pem_bytes).map_err(|e| {
            RqxError::TLSConfigError(format!("failed to construct client cert: {e}"))
        }))?
    }
}
