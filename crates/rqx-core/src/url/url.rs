use std::collections::hash_map::{DefaultHasher, HashMap};
use std::fmt;
use std::hash::{Hash, Hasher};

use super::components::{UrlComponentValue, UrlComponents};
use super::reference::UrlReference;
use crate::error::RqxError;
use crate::query_params::QueryPairs;

pub struct Url {
    reference: UrlReference,
}

// Initialization
impl Url {
    pub fn new(reference: UrlReference) -> Self {
        Self { reference }
    }

    fn with_params(&self, params: QueryPairs) -> Result<Self, RqxError> {
        Ok(Self::new(self.reference.with_params(&params)?))
    }

    // TODO: rename this to something better (is called py_new in the other impl)
    fn from_url_and_kwargs(url: String, kwargs: HashMap<String, Option<UrlComponentValue>>) {
        let reference = &UrlReference::parse(url.as_str())?;
        let components = UrlComponents::from_hash_map(kwargs)?;
        UrlReference::compose(Some(reference), components);
    }
}

// Getters
impl Url {
    fn scheme(&self) -> &str {
        self.reference.scheme()
    }

    fn username(&self) -> &str {
        self.reference.username()
    }

    fn password(&self) -> &str {
        self.reference.password()
    }

    fn host(&self) -> String {
        self.reference.host().into_owned()
    }

    fn port(&self) -> Option<u16> {
        self.reference.port()
    }

    fn path(&self) -> String {
        self.reference.path().into_owned()
    }

    fn query(&self) -> &[u8] {
        self.reference.query().as_bytes()
    }

    fn params(&self) -> QueryPairs {
        self.reference.params()
    }

    fn raw_path(&self) -> &[u8] {
        self.reference.raw_path().as_bytes()
    }

    fn fragment(&self) -> &str {
        self.reference.fragment()
    }

    fn is_absolute_url(&self) -> bool {
        self.reference.is_absolute()
    }

    fn is_relative_url(&self) -> bool {
        !self.reference.is_absolute()
    }
}

// Creating copies with different settings
impl Url {
    fn copy_with(
        &self,
        kwargs: Option<HashMap<String, Option<UrlComponentValue>>>,
    ) -> Result<Self, RqxError> {
        let components = match kwargs {
            Some(kwargs) => UrlComponents::extract(kwargs)?,
            None => UrlComponents::default(),
        };
        Ok(Self::new(UrlReference::compose(
            Some(&self.reference),
            components,
        )?))
    }

    fn copy_set_param(
        &self,
        key: &str,
        value: Option<UrlComponentValue>,
    ) -> Result<Self, RqxError> {
        self.with_params(
            self.reference
                .params()
                .set(key, QueryPairs::scalar_or_empty(value)?),
        )
    }

    fn copy_add_param(
        &self,
        key: &str,
        value: Option<UrlComponentValue>,
    ) -> Result<Self, RqxError> {
        self.with_params(
            self.reference
                .params()
                .add(key, QueryPairs::scalar_or_empty(value)?),
        )
    }

    fn copy_remove_param(&self, key: &str) -> Result<Self, RqxError> {
        self.with_params(self.reference.params().remove(key))
    }

    fn copy_merge_params(&self, params: Option<QueryPairs>) -> Result<Self, RqxError> {
        match params {
            Some(params) => self.with_params(self.reference.params().merge(&params)),
            None => Ok(Self::new(self.reference.clone())),
        }
    }

    fn join(&self, url: &str) -> Result<Self, RqxError> {
        Ok(Self::new(self.reference.join(url)?))
    }

    fn equals(&self, other: &str) -> bool {
        match UrlReference::parse(other) {
            Ok(other) => self.reference == other,
            Err(_) => false,
        }
    }

    fn hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.reference.to_string().hash(&mut hasher);
        hasher.finish()
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reference.to_string())
    }
}
