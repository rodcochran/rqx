use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

use super::components::{UrlComponentValue, UrlComponents};
use super::reference::UrlReference;
use crate::error::RqxError;
use crate::query_params::{QueryPairs, ScalarValue};

#[derive(Clone)]
pub struct RqxClientUrl {
    reference: UrlReference,
}

impl RqxClientUrl {
    pub fn new(reference: UrlReference) -> Self {
        Self { reference }
    }

    pub fn parse(input: &str) -> Result<Self, RqxError> {
        Ok(Self::new(UrlReference::parse(input)?))
    }

    fn with_params(&self, params: &QueryPairs) -> Result<Self, RqxError> {
        Ok(Self::new(self.reference.with_params(params)?))
    }

    pub fn get_inner(&self) -> UrlReference {
        self.reference.clone()
    }

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

    pub fn query(&self) -> &str {
        self.reference.query()
    }

    pub fn params(&self) -> QueryPairs {
        self.reference.params()
    }

    pub fn raw_path(&self) -> String {
        self.reference.raw_path()
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

    pub fn masked(&self) -> String {
        self.reference.masked()
    }

    pub fn copy_with(
        &self,
        kwargs: HashMap<String, Option<UrlComponentValue>>,
    ) -> Result<Self, RqxError> {
        if kwargs.is_empty() {
            return Ok(self.clone());
        }
        let components = UrlComponents::from_hash_map(kwargs)?;
        Ok(Self::new(UrlReference::compose(
            Some(&self.reference),
            components,
        )?))
    }

    pub fn copy_set_param(&self, key: &str, value: Option<ScalarValue>) -> Result<Self, RqxError> {
        self.with_params(&self.reference.params().set(key, QueryPairs::scalar(value)))
    }

    pub fn copy_add_param(&self, key: &str, value: Option<ScalarValue>) -> Result<Self, RqxError> {
        self.with_params(&self.reference.params().add(key, QueryPairs::scalar(value)))
    }

    pub fn copy_remove_param(&self, key: &str) -> Result<Self, RqxError> {
        self.with_params(&self.reference.params().remove(key))
    }

    pub fn copy_merge_params(&self, params: Option<QueryPairs>) -> Result<Self, RqxError> {
        match params {
            Some(params) => self.with_params(&self.reference.params().merge(&params)),
            None => Ok(self.clone()),
        }
    }

    pub fn join(&self, url: &str) -> Result<Self, RqxError> {
        Ok(Self::new(self.reference.join(url)?))
    }
}

impl fmt::Display for RqxClientUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reference)
    }
}

impl PartialEq for RqxClientUrl {
    fn eq(&self, other: &Self) -> bool {
        self.reference == other.reference
    }
}

impl Eq for RqxClientUrl {}

impl Hash for RqxClientUrl {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.reference.to_string().hash(state);
    }
}
