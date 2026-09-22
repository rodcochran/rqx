use std::fmt;
use std::hash::{Hash, Hasher};

use url::form_urlencoded;

pub enum ScalarValue {
    Bool(bool),
    String(String),
    Int(i64),
    Float(f64),
}

impl fmt::Display for ScalarValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScalarValue::Bool(v) => f.write_str(&v.to_string()),
            ScalarValue::String(v) => f.write_str(v),
            ScalarValue::Int(v) => f.write_str(&v.to_string()),
            ScalarValue::Float(v) => f.write_str(&v.to_string()),
        }
    }
}

#[derive(Clone, Default)]
pub struct QueryPairs(Vec<(String, String)>);

impl QueryPairs {
    pub fn pairs(&self) -> &[(String, String)] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn parse(query: &str) -> Self {
        let mut pairs = Self::default();
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            pairs.push(key.into_owned(), value.into_owned());
        }
        pairs
    }

    /// Values sit under the first appearance of their key, as they would in
    /// the `dict[str, list[str]]` httpx keeps.
    fn push(&mut self, key: String, value: String) {
        match self.0.iter().rposition(|(k, _)| *k == key) {
            Some(last) => self.0.insert(last + 1, (key, value)),
            None => self.0.push((key, value)),
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_list(&self, key: &str) -> Vec<&str> {
        self.0
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = Vec::new();
        for (key, _) in &self.0 {
            if !keys.contains(&key.as_str()) {
                keys.push(key);
            }
        }
        keys
    }

    pub fn first_items(&self) -> Vec<(&str, &str)> {
        self.keys()
            .into_iter()
            .map(|key| (key, self.get(key).unwrap_or_default()))
            .collect()
    }

    pub fn set(&self, key: &str, value: String) -> Self {
        let mut out = Vec::with_capacity(self.0.len() + 1);
        let mut replaced = false;
        for (k, v) in &self.0 {
            if k != key {
                out.push((k.clone(), v.clone()));
            } else if !replaced {
                out.push((key.to_owned(), value.clone()));
                replaced = true;
            }
        }
        if !replaced {
            out.push((key.to_owned(), value));
        }
        Self(out)
    }

    pub fn add(&self, key: &str, value: String) -> Self {
        let mut out = self.clone();
        out.push(key.to_owned(), value);
        out
    }

    pub fn remove(&self, key: &str) -> Self {
        Self(self.0.iter().filter(|(k, _)| k != key).cloned().collect())
    }

    /// `other` wins for any key it carries, in place; its new keys go last.
    pub fn merge(&self, other: &Self) -> Self {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut taken: Vec<&str> = Vec::new();
        for (key, value) in &self.0 {
            if !other.contains(key) {
                out.push((key.clone(), value.clone()));
                continue;
            }
            if !taken.contains(&key.as_str()) {
                taken.push(key);
                out.extend(
                    other
                        .get_list(key)
                        .iter()
                        .map(|v| (key.clone(), (*v).to_owned())),
                );
            }
        }
        for (key, value) in &other.0 {
            if !taken.contains(&key.as_str()) {
                out.push((key.clone(), value.clone()));
            }
        }
        Self(out)
    }

    /// Equality ignores order, within a key as well as across keys, so the
    /// hash is built from the same thing.
    fn sorted(&self) -> Vec<&(String, String)> {
        let mut pairs: Vec<&(String, String)> = self.0.iter().collect();
        pairs.sort();
        pairs
    }

    pub fn from_items(items: Vec<(String, Vec<Option<ScalarValue>>)>) -> Self {
        let mut pairs = Self::default();
        for (key, values) in items {
            for value in values {
                pairs.push(key.clone(), Self::scalar(value));
            }
        }
        pairs
    }

    pub fn scalar(value: Option<ScalarValue>) -> String {
        match value {
            Some(v) => v.to_string(),
            None => String::new(),
        }
    }
}

impl fmt::Display for QueryPairs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut encoded = form_urlencoded::Serializer::new(String::new());
        for (key, value) in &self.0 {
            encoded.append_pair(key, value);
        }
        f.write_str(&encoded.finish())
    }
}

impl PartialEq for QueryPairs {
    fn eq(&self, other: &Self) -> bool {
        self.sorted() == other.sorted()
    }
}

impl Eq for QueryPairs {}

impl Hash for QueryPairs {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.sorted().hash(state);
    }
}
