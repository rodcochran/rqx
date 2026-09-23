use http::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use crate::error::*;

#[derive(Clone, PartialEq)]
pub struct Headers {
    pub(crate) inner: HeaderMap,
}

impl Headers {
    pub fn new(raw_headers: Option<HashMap<String, String>>) -> Result<Self, RqxError> {
        match raw_headers {
            Some(map) => Self::try_from_pairs(map.into_iter().collect()),
            None => Ok(Self::from_header_map(HeaderMap::new())),
        }
    }

    pub fn try_from_pairs(pairs: Vec<(String, String)>) -> Result<Self, RqxError> {
        let mut inner = HeaderMap::try_with_capacity(pairs.len()).unwrap_or_default();
        for (key, value) in pairs {
            let name = match HeaderName::from_str(&key) {
                Ok(name) => name,
                Err(e) => return Err(HeaderError::InvalidName(format!("{key:?}: {e}")).into()),
            };
            let value = match HeaderValue::from_str(&value) {
                Ok(value) => value,
                Err(e) => return Err(HeaderError::InvalidValue(format!("{value:?}: {e}")).into()),
            };
            inner.try_append(name, value)?;
        }
        Ok(Self { inner })
    }

    pub fn from_header_map(header_map: HeaderMap) -> Self {
        Self { inner: header_map }
    }

    /// Return the first value matching `key` (case-insensitive). Used by
    /// Rust-side code that just wants a single header for internal logic.
    pub fn get_first(&self, key: &str) -> Option<&str> {
        self.inner.get(key)?.to_str().ok()
    }

    /// Every value for `key`, joined with `, ` the way httpx presents them.
    pub fn get_joined_values_for_key(&self, key: &str) -> Result<String, RqxError> {
        let values: Vec<&str> = self
            .inner
            .get_all(key)
            .iter()
            .map(|v| v.to_str().unwrap_or(""))
            .collect();
        match values.is_empty() {
            true => Err(HeaderError::MissingKey(key.to_string()).into()),
            false => Ok(values.join(", ")),
        }
    }

    pub fn set_item(&mut self, key: &str, value: String) -> Result<(), RqxError> {
        let name = HeaderName::from_str(key)?;
        let val = HeaderValue::from_str(&value)?;
        // Replaces existing entries with this name.
        self.inner.try_insert(name, val)?;
        Ok(())
    }

    pub fn delete_item(&mut self, key: &str) -> Result<(), RqxError> {
        match self.inner.remove(key) {
            Some(_) => Ok(()),
            None => Err(HeaderError::MissingKey(key.to_string()).into()),
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.inner.contains_key(key)
    }

    pub fn length(&self) -> usize {
        self.inner.keys_len()
    }

    pub fn keys(&self) -> Vec<String> {
        self.inner.keys().map(|k| k.as_str().to_string()).collect()
    }

    pub fn values(&self) -> Vec<String> {
        self.inner
            .values()
            .map(|v| v.to_str().unwrap_or("").to_string())
            .collect()
    }

    pub fn items(&self) -> Vec<(String, String)> {
        // Includes duplicates (Set-Cookie, etc.) — same as iterating HeaderMap directly.
        self.inner
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect()
    }
}

impl fmt::Debug for Headers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.inner)
    }
}
