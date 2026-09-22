use bytes::Bytes;
use std::collections::HashMap;

use crate::error::*;
use crate::query_params::QueryPairs;

pub enum UrlComponentValue {
    String(String),
    Bytes(Bytes),
    Int(u16),
    QueryPairs(QueryPairs),
}

impl std::fmt::Display for UrlComponentValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => f.write_str(s),
            Self::Bytes(b) => f.write_str(&String::from_utf8_lossy(b)),
            Self::Int(i) => write!(f, "{i}"),
            Self::QueryPairs(qp) => write!(f, "{qp}"),
        }
    }
}

impl UrlComponentValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::Int(_) => "int",
            Self::QueryPairs(_) => "QueryPairs",
        }
    }
}
#[derive(Default)]
pub struct UrlComponents {
    pub scheme: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<Option<u16>>,
    pub path: Option<String>,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

impl UrlComponents {
    pub fn from_hash_map(
        map: HashMap<String, Option<UrlComponentValue>>,
    ) -> Result<Self, RqxError> {
        let mut components = Self::default();
        for (key, value) in &map {
            match key.as_str() {
                "scheme" => components = components.with_scheme(value),
                "username" => components = components.with_username(value),
                "password" => components = components.with_password(value),
                "host" => components = components.with_host(value),
                "port" => components = components.with_port(value)?,
                "path" => components = components.with_path(value),
                "query" => components = components.with_query(value),
                "fragment" => components = components.with_fragment(value),
                "params" => components = components.with_params(value),
                k => {
                    return Err(RqxError::UnknownKeyword(format!(
                        "'{}' is an invalid keyword argument for URL()",
                        k
                    )));
                }
            }
        }
        Ok(components)
    }

    // `None` clears the component; `compose` reads an empty string as "not set".
    fn component_field_str(v: &Option<UrlComponentValue>) -> Option<String> {
        match v {
            Some(value) => Some(value.to_string()),
            None => Some(String::new()),
        }
    }

    fn component_field_u16(v: &Option<UrlComponentValue>) -> Result<Option<u16>, RqxError> {
        match v {
            None => Ok(None),
            Some(UrlComponentValue::Int(n)) => Ok(Some(*n)),
            Some(_v) => Err(RqxError::InvalidURL(format!(
                "Component value must be u16 for u16 field, but got {}",
                _v.type_name()
            ))),
        }
    }

    fn with_scheme(mut self, v: &Option<UrlComponentValue>) -> Self {
        self.scheme = Self::component_field_str(v);
        self
    }

    fn with_username(mut self, v: &Option<UrlComponentValue>) -> Self {
        self.username = Self::component_field_str(v);
        self
    }

    fn with_password(mut self, v: &Option<UrlComponentValue>) -> Self {
        self.password = Self::component_field_str(v);
        self
    }

    fn with_host(mut self, v: &Option<UrlComponentValue>) -> Self {
        self.host = Self::component_field_str(v);
        self
    }

    fn with_port(mut self, v: &Option<UrlComponentValue>) -> Result<Self, RqxError> {
        self.port = Some(Self::component_field_u16(v)?);
        Ok(self)
    }

    fn with_path(mut self, v: &Option<UrlComponentValue>) -> Self {
        self.path = Self::component_field_str(v);
        self
    }

    fn with_query(mut self, v: &Option<UrlComponentValue>) -> Self {
        self.query = Self::component_field_str(v);
        self
    }

    fn with_fragment(mut self, v: &Option<UrlComponentValue>) -> Self {
        self.fragment = Self::component_field_str(v);
        self
    }

    fn with_params(mut self, v: &Option<UrlComponentValue>) -> Self {
        self.query = Self::component_field_str(v);
        self
    }
}
