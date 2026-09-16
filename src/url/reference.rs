//! `rqx.URL`: absolute URLs and relative references, httpx's semantics
//! (https://github.com/rodcochran/rqx/issues/59).

use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

use percent_encoding::percent_decode_str;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyString};
use url::{ParseError, Url};

use crate::exceptions::InvalidURL;
use crate::query_params::QueryPairs;

/// A relative reference has no authority for `url::Url` to hold, so it is
/// resolved against a base that can't collide with a real one and stripped
/// back out on the way to a string. `.invalid` is reserved by RFC 2606.
static RELATIVE_BASE: LazyLock<Url> =
    LazyLock::new(|| Url::parse("http://rqx.invalid/").expect("static base URL parses"));

#[derive(Clone, Copy, PartialEq)]
enum RelativeShape {
    /// `//host/path` — an authority but no scheme.
    Network,
    /// `/path`
    Rooted,
    /// `path`, `../path`
    Bare,
    /// ``, `?x=1`, `#frag` — the base's `/` is an artifact, not content.
    NoPath,
}

impl RelativeShape {
    fn of(input: &str) -> Self {
        match input.as_bytes() {
            [b'/', b'/', ..] => Self::Network,
            [b'/', ..] => Self::Rooted,
            [] | [b'?', ..] | [b'#', ..] => Self::NoPath,
            _ => Self::Bare,
        }
    }
}

#[derive(Clone)]
pub struct UrlReference {
    url: Url,
    shape: Option<RelativeShape>,
}

impl UrlReference {
    pub fn parse(input: &str) -> PyResult<Self> {
        match Url::parse(input) {
            Ok(url) => Ok(Self { url, shape: None }),
            Err(ParseError::RelativeUrlWithoutBase) => Ok(Self {
                url: RELATIVE_BASE.join(input).map_err(Self::invalid(input))?,
                shape: Some(RelativeShape::of(input)),
            }),
            Err(e) => Err(Self::invalid(input)(e)),
        }
    }

    pub fn from_url(url: Url) -> Self {
        Self { url, shape: None }
    }

    fn invalid(input: &str) -> impl Fn(ParseError) -> pyo3::PyErr {
        let input = input.to_owned();
        move |e| InvalidURL::new_err(format!("invalid URL {input:?}: {e}"))
    }

    /// httpx's rule: a scheme and a host, or it's a reference to somewhere else.
    pub fn is_absolute(&self) -> bool {
        self.shape.is_none() && self.url.host().is_some()
    }

    pub fn scheme(&self) -> &str {
        match self.shape {
            None => self.url.scheme(),
            Some(_) => "",
        }
    }

    /// Percent-encoded, as it appears in the URL: `copy_with` round-trips it
    /// and `__repr__` masks it by position.
    pub fn username(&self) -> &str {
        match self.shape {
            None => self.url.username(),
            Some(_) => "",
        }
    }

    pub fn password(&self) -> &str {
        match self.shape {
            None => self.url.password().unwrap_or_default(),
            Some(_) => "",
        }
    }

    /// The unicode form: `str(url)` carries punycode, `url.host` doesn't.
    pub fn host(&self) -> Cow<'_, str> {
        let Some(host) = self.encoded_host() else {
            return Cow::Borrowed("");
        };
        match idna::domain_to_unicode(host) {
            (unicode, Ok(())) => Cow::Owned(unicode),
            (_, Err(_)) => Cow::Borrowed(host),
        }
    }

    fn encoded_host(&self) -> Option<&str> {
        match self.shape {
            None | Some(RelativeShape::Network) => self.url.host_str(),
            Some(_) => None,
        }
    }

    /// Normalized: a default port for the scheme reads as no port at all.
    pub fn port(&self) -> Option<u16> {
        match self.shape {
            None => self.url.port(),
            Some(_) => None,
        }
    }

    /// Still percent-encoded, with the base's leading `/` removed where the
    /// original didn't have one.
    pub fn encoded_path(&self) -> &str {
        let path = self.url.path();
        match self.shape {
            Some(RelativeShape::Bare) => path.trim_start_matches('/'),
            _ => path,
        }
    }

    pub fn path(&self) -> Cow<'_, str> {
        percent_decode_str(self.encoded_path()).decode_utf8_lossy()
    }

    pub fn query(&self) -> &str {
        self.url.query().unwrap_or_default()
    }

    pub fn params(&self) -> QueryPairs {
        QueryPairs::parse(self.query())
    }

    pub fn fragment(&self) -> &str {
        self.url.fragment().unwrap_or_default()
    }

    pub fn raw_path(&self) -> String {
        let mut raw = self.encoded_path().to_owned();
        if let Some(query) = self.url.query() {
            raw.push('?');
            raw.push_str(query);
        }
        raw
    }

    pub fn with_query(&self, query: Option<&str>) -> Self {
        let mut url = self.url.clone();
        url.set_query(query.filter(|q| !q.is_empty()));
        Self {
            url,
            shape: self.shape,
        }
    }

    pub fn with_params(&self, params: &QueryPairs) -> Self {
        self.with_query(Some(&params.to_string()))
    }

    /// Resolve a reference against this URL. An absolute argument wins
    /// outright; anything else keeps this URL's shape.
    pub fn join(&self, other: &str) -> PyResult<Self> {
        let joined = self.url.join(other).map_err(Self::invalid(other))?;
        let shape = match Url::parse(other) {
            Ok(_) => None,
            Err(_) => self.shape,
        };
        Ok(Self { url: joined, shape })
    }

    pub fn compose(base: Option<&Self>, components: UrlComponents) -> PyResult<Self> {
        let current = |read: fn(&Self) -> &str| base.map(read).unwrap_or_default().to_owned();

        let scheme = components.scheme.unwrap_or_else(|| current(Self::scheme));
        let username = components
            .username
            .unwrap_or_else(|| current(Self::username));
        let password = components
            .password
            .unwrap_or_else(|| current(Self::password));
        let host = components.host.unwrap_or_else(|| {
            base.and_then(Self::encoded_host)
                .unwrap_or_default()
                .to_owned()
        });
        let port = components.port.unwrap_or_else(|| base.and_then(Self::port));
        let path = components
            .path
            .unwrap_or_else(|| base.map(Self::encoded_path).unwrap_or_default().to_owned());
        let query = components.query.unwrap_or_else(|| match base {
            Some(base) => base.url.query().map(str::to_owned),
            None => None,
        });
        let fragment = components.fragment.unwrap_or_else(|| match base {
            Some(base) => base.url.fragment().map(str::to_owned),
            None => None,
        });

        let mut composed = String::new();
        if !host.is_empty() {
            if !scheme.is_empty() {
                composed.push_str(&scheme);
                composed.push(':');
            }
            composed.push_str("//");
            if !password.is_empty() {
                composed.push_str(&format!("{username}:{password}@"));
            } else if !username.is_empty() {
                composed.push_str(&format!("{username}@"));
            }
            composed.push_str(&host);
            if let Some(port) = port {
                composed.push_str(&format!(":{port}"));
            }
        }
        if !host.is_empty() && !path.is_empty() && !path.starts_with('/') {
            composed.push('/');
        }
        composed.push_str(&path);
        if let Some(query) = query.filter(|q| !q.is_empty()) {
            composed.push('?');
            composed.push_str(&query);
        }
        if let Some(fragment) = fragment.filter(|f| !f.is_empty()) {
            composed.push('#');
            composed.push_str(&fragment);
        }
        Self::parse(&composed)
    }
}

impl fmt::Display for UrlReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(shape) = self.shape else {
            return f.write_str(self.url.as_str());
        };
        if shape == RelativeShape::Network {
            write!(f, "//{}", self.url.authority())?;
        }
        if shape != RelativeShape::NoPath {
            f.write_str(self.encoded_path())?;
        }
        if let Some(query) = self.url.query() {
            write!(f, "?{query}")?;
        }
        if let Some(fragment) = self.url.fragment() {
            write!(f, "#{fragment}")?;
        }
        Ok(())
    }
}

impl PartialEq for UrlReference {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
}

/// The pieces `URL(**kwargs)` and `copy_with(**kwargs)` can set. `None` is
/// "not given, keep what's there"; the caller turns an explicit Python `None`
/// into a cleared value.
#[derive(Default)]
pub struct UrlComponents {
    pub scheme: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<Option<u16>>,
    pub path: Option<String>,
    pub query: Option<Option<String>>,
    pub fragment: Option<Option<String>>,
}

impl UrlComponents {
    pub fn extract(kwargs: &Bound<'_, PyDict>) -> PyResult<Self> {
        let mut components = Self::default();
        for (key, value) in kwargs.iter() {
            match key.extract::<String>()?.as_str() {
                "scheme" => components.scheme = Some(Self::text(&value)?),
                "username" => components.username = Some(Self::text(&value)?),
                "password" => components.password = Some(Self::text(&value)?),
                "host" => components.host = Some(Self::text(&value)?),
                "port" => {
                    components.port = Some(match value.is_none() {
                        true => None,
                        false => Some(value.extract()?),
                    })
                }
                "path" => components.path = Some(Self::text(&value)?),
                "query" => components.query = Some(Self::optional_text(&value)?),
                "fragment" => components.fragment = Some(Self::optional_text(&value)?),
                "params" => {
                    components.query = Some(Some(value.extract::<QueryPairs>()?.to_string()))
                }
                key => {
                    return Err(PyTypeError::new_err(format!(
                        "'{key}' is an invalid keyword argument for URL()"
                    )));
                }
            }
        }
        Ok(components)
    }

    fn optional_text(value: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
        Ok(Some(Self::text(value)?).filter(|text| !text.is_empty()))
    }

    fn text(value: &Bound<'_, PyAny>) -> PyResult<String> {
        if value.is_none() {
            return Ok(String::new());
        }
        if let Ok(text) = value.cast::<PyString>() {
            return Ok(text.to_cow()?.into_owned());
        }
        if let Ok(raw) = value.cast::<PyBytes>() {
            return Ok(String::from_utf8_lossy(raw.as_bytes()).into_owned());
        }
        Err(PyTypeError::new_err(format!(
            "URL components must be str, bytes, or None, got {}",
            value.get_type().name()?
        )))
    }
}
