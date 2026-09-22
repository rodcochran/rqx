use http::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::HashMap;
use std::str::FromStr;

use crate::error::*;

pub struct Headers {
    pub inner: HeaderMap,
}

impl Headers {
    pub fn new(raw_headers: Option<HashMap<String, String>>) -> Result<Self, RqxError> {
        let mut inner = HeaderMap::new();

        if let Some(map) = raw_headers {
            for (k, v) in map {
                let name = HeaderName::from_str(&k)?;
                let value = HeaderValue::from_str(&v)?;
                inner.try_insert(name, value)?;
            }
        }
        Ok(Self { inner })
    }
}

impl Headers {
    /// Build from `Vec<(name, value)>` — used by response construction where
    /// the data came from reqwest's iteration.
    pub fn from_pairs(items: Vec<(String, String)>) -> Self {
        let mut inner = HeaderMap::try_with_capacity(items.len()).unwrap_or_default();
        for (k, v) in items {
            // Skip malformed names/values defensively. reqwest's HeaderMap
            // shouldn't ever produce them, but we don't want to panic if
            // something pathological slips through. Same for the entry cap.
            if let (Ok(name), Ok(value)) = (HeaderName::from_str(&k), HeaderValue::from_str(&v)) {
                let _ = inner.try_append(name, value); // append preserves multi-values
            }
        }
        Self { inner }
    }

    /// Return the first value matching `key` (case-insensitive). Used by
    /// Rust-side code that just wants a single header for internal logic.
    pub fn get_first(&self, key: &str) -> Option<&str> {
        self.inner.get(key)?.to_str().ok()
    }

    pub fn from_header_map(header_map: HeaderMap) -> Self {
        Self { inner: header_map }
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
}

impl Headers {
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
