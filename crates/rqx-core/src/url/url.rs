use std::collections::hash_map::{DefaultHasher, HashMap};
use std::fmt;
use std::hash::{Hash, Hasher};

use super::components::{UrlComponentValue, UrlComponents};
use super::reference::UrlReference;
use crate::error::RqxError;
use crate::query_params::{QueryPairs, ScalarValue};

pub struct RqxClientUrl {
    reference: UrlReference,
}

// Initialization
impl RqxClientUrl {
    pub fn new(reference: UrlReference) -> Self {
        Self { reference }
    }

    pub fn with_params(&self, params: QueryPairs) -> Result<Self, RqxError> {
        Ok(Self::new(self.reference.with_params(&params)?))
    }

    // TODO: rename this to something better (is called py_new in the other impl)
    pub fn from_url_and_kwargs(
        url: String,
        kwargs: HashMap<String, Option<UrlComponentValue>>,
    ) -> Result<UrlReference, RqxError> {
        let reference = &UrlReference::parse(url.as_str())?;
        let components = UrlComponents::from_hash_map(kwargs)?;
        UrlReference::compose(Some(reference), components)
    }
}

// Getters
impl RqxClientUrl {
    pub fn scheme(&self) -> &str {
        self.reference.scheme()
    }

    pub fn username(&self) -> &str {
        self.reference.username()
    }

    pub fn password(&self) -> &str {
        self.reference.password()
    }

    pub fn host(&self) -> String {
        self.reference.host().into_owned()
    }

    pub fn port(&self) -> Option<u16> {
        self.reference.port()
    }

    pub fn path(&self) -> String {
        self.reference.path().into_owned()
    }

    pub fn query(&self) -> &[u8] {
        self.reference.query().as_bytes()
    }

    pub fn params(&self) -> QueryPairs {
        self.reference.params()
    }

    pub fn raw_path(&self) -> Vec<u8> {
        self.reference.raw_path().as_bytes().to_owned()
    }

    pub fn fragment(&self) -> &str {
        self.reference.fragment()
    }

    pub fn is_absolute_url(&self) -> bool {
        self.reference.is_absolute()
    }

    pub fn is_relative_url(&self) -> bool {
        !self.reference.is_absolute()
    }
}

// Creating copies with different settings
impl RqxClientUrl {
    pub fn copy_with(
        &self,
        kwargs: HashMap<String, Option<UrlComponentValue>>,
    ) -> Result<Self, RqxError> {
        let components = UrlComponents::from_hash_map(kwargs)?;
        Ok(Self::new(UrlReference::compose(
            Some(&self.reference),
            components,
        )?))
    }

    pub fn copy_set_param(&self, key: &str, value: Option<ScalarValue>) -> Result<Self, RqxError> {
        self.with_params(self.reference.params().set(key, QueryPairs::scalar(value)))
    }

    pub fn copy_add_param(&self, key: &str, value: Option<ScalarValue>) -> Result<Self, RqxError> {
        self.with_params(self.reference.params().add(key, QueryPairs::scalar(value)))
    }

    pub fn copy_remove_param(&self, key: &str) -> Result<Self, RqxError> {
        self.with_params(self.reference.params().remove(key))
    }

    pub fn copy_merge_params(&self, params: Option<QueryPairs>) -> Result<Self, RqxError> {
        match params {
            Some(params) => self.with_params(self.reference.params().merge(&params)),
            None => Ok(Self::new(self.reference.clone())),
        }
    }

    pub fn join(&self, url: &str) -> Result<Self, RqxError> {
        Ok(Self::new(self.reference.join(url)?))
    }

    pub fn equals(&self, other: &str) -> bool {
        match UrlReference::parse(other) {
            Ok(other) => self.reference == other,
            Err(_) => false,
        }
    }

    pub fn hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.reference.to_string().hash(&mut hasher);
        hasher.finish()
    }

    pub fn masked(&self) -> String {
        self.reference.masked()
    }
}

impl fmt::Display for RqxClientUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reference.to_string())
    }
}
