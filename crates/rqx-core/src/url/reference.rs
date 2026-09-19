//! `rqx.URL`: absolute URLs and relative references, httpx's semantics
//! (https://github.com/rodcochran/rqx/issues/59).

use std::borrow::Cow;
use std::fmt;

use iri_string::components::AuthorityComponents;
use iri_string::percent_encode::PercentEncoded;
use iri_string::spec::UriSpec;
use iri_string::types::{UriReferenceStr, UriRelativeStr, UriRelativeString};
use percent_encoding::percent_decode_str;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyString};
use url::{ParseError, Url};

use crate::error::*;
use crate::query_params::QueryPairs;

/// `url::Url` gives WHATWG normalization — default ports dropped, hosts
/// lowercased and punycoded, paths percent-encoded — but can't hold a URL
/// without a host. `iri-string` holds a relative reference exactly as
/// written. Each half does what it's good at.
#[derive(Clone)]
pub enum UrlReference {
    Absolute(Url),
    Relative(UriRelativeString),
}

impl UrlReference {
    pub fn parse(input: &str) -> Result<Self, RqxError> {
        match Url::parse(input) {
            Ok(url) => Ok(Self::Absolute(url)),
            Err(ParseError::RelativeUrlWithoutBase) => Self::parse_relative(input),
            Err(e) => Err(
                Self::invalid(
                    input, &e,
                ),
            ),
        }
    }

    pub fn from_url(url: Url) -> Self {
        Self::Absolute(url)
    }

    fn parse_relative(input: &str) -> Result<Self, RqxError> {
        if let Ok(reference) = UriRelativeStr::new(input) {
            return Ok(Self::Relative(reference.to_owned()));
        }
        // A character RFC 3986 won't take raw — a space, say — is encoded
        // rather than rejected, which is what httpx does with it.
        let encoded = Self::encode_relative(input);
        match UriRelativeStr::new(&encoded) {
            Ok(reference) => Ok(Self::Relative(reference.to_owned())),
            Err(e) => Err(
                Self::invalid(
                    input, &e,
                ),
            ),
        }
    }

    /// An authority is IDNA, not percent-encoding, so a network-path
    /// reference borrows `url::Url`'s authority parser — userinfo, IPv6 and
    /// all — by parsing under a borrowed scheme.
    ///
    /// A scheme drops its own default port, and a reference with no scheme has
    /// no default to drop, so it is parsed under two schemes with different
    /// defaults and the one that kept the port wins.
    fn encode_relative(input: &str) -> String {
        let Some(rest) = input.strip_prefix("//") else {
            return Self::encode_path(input);
        };
        let (Ok(ftp), Ok(ws)) = (
            Url::parse(&format!("ftp:{input}")),
            Url::parse(&format!("ws:{input}")),
        ) else {
            return Self::encode_path(input);
        };
        let parsed = match (
            ftp.port(),
            ws.port(),
        ) {
            (None, Some(_)) => ws,
            _ => ftp,
        };

        let encoded = parsed
            .as_str()
            .trim_start_matches(parsed.scheme())
            .trim_start_matches(':');
        // `Url` always has a path; the reference it came from need not.
        match rest.contains(['/', '?', '#']) {
            true => encoded.to_owned(),
            false => encoded.trim_end_matches('/').to_owned(),
        }
    }

    fn encode_path(input: &str) -> String {
        let (head, fragment) = match input.split_once('#') {
            Some((head, fragment)) => (
                head,
                Some(fragment),
            ),
            None => (
                input, None,
            ),
        };
        let (path, query) = match head.split_once('?') {
            Some((path, query)) => (
                path,
                Some(query),
            ),
            None => (
                head, None,
            ),
        };

        let mut encoded = PercentEncoded::<_, UriSpec>::from_path(path).to_string();
        if let Some(query) = query {
            encoded.push('?');
            encoded.push_str(&PercentEncoded::<_, UriSpec>::from_query(query).to_string());
        }
        if let Some(fragment) = fragment {
            encoded.push('#');
            encoded.push_str(&PercentEncoded::<_, UriSpec>::from_fragment(fragment).to_string());
        }
        encoded
    }

    fn invalid(input: &str, error: &dyn fmt::Display) -> RqxError {
        RqxError::InvalidURL(format!("invalid URL {input:?}: {error}")).into()
    }

    /// httpx's rule: a scheme and a host, or it's a reference to somewhere else.
    pub fn is_absolute(&self) -> bool {
        matches!(self, Self::Absolute(url) if url.host().is_some())
    }

    fn authority(&self) -> Option<AuthorityComponents<'_>> {
        match self {
            Self::Absolute(_) => None,
            Self::Relative(reference) => reference.authority_components(),
        }
    }

    pub fn scheme(&self) -> &str {
        match self {
            Self::Absolute(url) => url.scheme(),
            Self::Relative(_) => "",
        }
    }

    /// Percent-encoded, as it appears in the URL: `copy_with` round-trips it
    /// and `__repr__` masks it.
    pub fn username(&self) -> &str {
        match self {
            Self::Absolute(url) => url.username(),
            Self::Relative(_) => self.userinfo().0,
        }
    }

    pub fn password(&self) -> &str {
        match self {
            Self::Absolute(url) => url.password().unwrap_or_default(),
            Self::Relative(_) => self.userinfo().1,
        }
    }

    fn userinfo(
        &self,
    ) -> (
        &str,
        &str,
    ) {
        let Some(userinfo) = self.authority().and_then(|authority| authority.userinfo()) else {
            return (
                "", "",
            );
        };
        userinfo.split_once(':').unwrap_or((
            userinfo, "",
        ))
    }

    /// The unicode form: `str(url)` carries punycode, `url.host` doesn't.
    /// An IPv6 literal loses the brackets that delimit it in the URL.
    pub fn host(&self) -> Cow<'_, str> {
        let host = self.encoded_host();
        if let Some(literal) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            return Cow::Borrowed(literal);
        }
        if host.is_empty() {
            return Cow::Borrowed("");
        }
        match idna::domain_to_unicode(host) {
            (unicode, Ok(())) => Cow::Owned(unicode),
            (_, Err(_)) => Cow::Borrowed(host),
        }
    }

    fn encoded_host(&self) -> &str {
        match self {
            Self::Absolute(url) => url.host_str().unwrap_or_default(),
            Self::Relative(_) => self.authority().map_or(
                "",
                |authority| authority.host(),
            ),
        }
    }

    /// Normalized: a default port for the scheme reads as no port at all.
    pub fn port(&self) -> Option<u16> {
        match self {
            Self::Absolute(url) => url.port(),
            Self::Relative(_) => self
                .authority()
                .and_then(|authority| authority.port())
                .and_then(|port| port.parse().ok()),
        }
    }

    /// As written, still percent-encoded. Empty for a reference that is all
    /// query or fragment, where `path` and `raw_path` report `/` instead.
    pub fn encoded_path(&self) -> &str {
        match self {
            Self::Absolute(url) => url.path(),
            Self::Relative(reference) => reference.path_str(),
        }
    }

    fn encoded_path_or_root(&self) -> &str {
        match self.encoded_path() {
            "" => "/",
            path => path,
        }
    }

    pub fn path(&self) -> Cow<'_, str> {
        percent_decode_str(self.encoded_path_or_root()).decode_utf8_lossy()
    }

    /// What goes on the request line: path plus query, no authority.
    pub fn raw_path(&self) -> String {
        let mut raw = self.encoded_path_or_root().to_owned();
        let query = self.query();
        if !query.is_empty() {
            raw.push('?');
            raw.push_str(query);
        }
        raw
    }

    pub fn query(&self) -> &str {
        match self {
            Self::Absolute(url) => url.query().unwrap_or_default(),
            Self::Relative(reference) => reference.query_str().unwrap_or_default(),
        }
    }

    pub fn params(&self) -> QueryPairs {
        QueryPairs::parse(self.query())
    }

    pub fn fragment(&self) -> &str {
        match self {
            Self::Absolute(url) => url.fragment().unwrap_or_default(),
            Self::Relative(reference) => reference.fragment_str().unwrap_or_default(),
        }
    }

    pub fn with_params(&self, params: &QueryPairs) -> PyResult<Self> {
        Self::compose(
            Some(self),
            UrlComponents {
                query: Some(params.to_string()),
                ..UrlComponents::default()
            },
        )
    }

    /// `__repr__`'s form, with any password replaced.
    pub fn masked(&self) -> String {
        let text = self.to_string();
        match UriReferenceStr::new(&text) {
            Ok(reference) => reference
                .mask_password()
                .replace_password("[secure]")
                .to_string(),
            Err(_) => text,
        }
    }

    /// Resolve a reference against this URL. An absolute argument wins outright.
    pub fn join(&self, other: &str) -> Result<Self, RqxError> {
        // An empty reference resolves to the base as it stands, fragment and
        // all — what httpx and `urllib.parse.urljoin` both do. `Url::join`
        // drops the fragment here.
        if other.is_empty() {
            return Ok(self.clone());
        }
        match self {
            Self::Absolute(url) => Ok(
                Self::Absolute(
                    url.join(other).map_err(
                        |e| {
                            Self::invalid(
                                other, &e,
                            )
                        },
                    )?,
                ),
            ),
            Self::Relative(reference) => Self::join_relative(
                reference, other,
            ),
        }
    }

    /// RFC 3986 §5.3: an argument carrying its own scheme or authority
    /// replaces this reference outright. Anything else merges against it, on
    /// a borrowed absolute base — resolution needs one — that the result is
    /// then taken back off. `.invalid` is reserved by RFC 2606.
    fn join_relative(reference: &UriRelativeStr, other: &str) -> Result<Self, RqxError> {
        const ANCHOR: &str = "http://rqx.invalid";

        if Url::parse(other).is_ok() || other.starts_with("//") {
            return Self::parse(other);
        }

        // The query is anchored too: RFC 3986 §5.3 keeps the base's query for
        // an empty or fragment-only reference.
        let path = reference.path_str();
        let mut anchored_base = format!(
            "{ANCHOR}/{}",
            path.trim_start_matches('/')
        );
        if let Some(query) = reference.query_str() {
            anchored_base.push('?');
            anchored_base.push_str(query);
        }
        let anchored = Url::parse(&anchored_base).map_err(
            |e| {
                Self::invalid(
                    reference.as_str(),
                    &e,
                )
            },
        )?;
        let joined = anchored.join(other).map_err(
            |e| {
                Self::invalid(
                    other, &e,
                )
            },
        )?;
        let tail = joined
            .as_str()
            .strip_prefix(ANCHOR)
            .unwrap_or_else(|| joined.as_str());

        // The receiver's own authority survives; without one, the result is
        // rooted only if either side was.
        let rebuilt = match reference.authority_str() {
            Some(authority) => format!("//{authority}{tail}"),
            None if path.starts_with('/') || other.starts_with('/') => tail.to_owned(),
            None => tail.trim_start_matches('/').to_owned(),
        };
        Self::parse(&rebuilt)
    }

    /// Rebuild from components, letting the parser do the encoding.
    pub fn compose(base: Option<&Self>, components: UrlComponents) -> PyResult<Self> {
        let current = |read: fn(&Self) -> &str| base.map(read).unwrap_or_default().to_owned();

        let scheme = components.scheme.unwrap_or_else(|| current(Self::scheme));
        let username = components
            .username
            .unwrap_or_else(|| current(Self::username));
        let password = components
            .password
            .unwrap_or_else(|| current(Self::password));
        let host = components
            .host
            .unwrap_or_else(|| current(Self::encoded_host));
        let port = components.port.unwrap_or_else(|| base.and_then(Self::port));
        let path = components
            .path
            .unwrap_or_else(|| current(Self::encoded_path));
        let query = components.query.unwrap_or_else(|| current(Self::query));
        let fragment = components
            .fragment
            .unwrap_or_else(|| current(Self::fragment));

        let mut composed = String::new();
        if !host.is_empty() {
            if !scheme.is_empty() {
                composed.push_str(&scheme);
                composed.push(':');
            }
            composed.push_str("//");
            if !username.is_empty() || !password.is_empty() {
                composed.push_str(&username);
                if !password.is_empty() {
                    composed.push(':');
                    composed.push_str(&password);
                }
                composed.push('@');
            }
            composed.push_str(&host);
            if let Some(port) = port {
                composed.push(':');
                composed.push_str(&port.to_string());
            }
            if !path.is_empty() && !path.starts_with('/') {
                composed.push('/');
            }
        }
        composed.push_str(&path);
        if !query.is_empty() {
            composed.push('?');
            composed.push_str(&query);
        }
        if !fragment.is_empty() {
            composed.push('#');
            composed.push_str(&fragment);
        }
        Self::parse(&composed)
    }
}

impl fmt::Display for UrlReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute(url) => f.write_str(url.as_str()),
            Self::Relative(reference) => f.write_str(reference.as_str()),
        }
    }
}

impl PartialEq for UrlReference {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
}

/// The pieces `URL(**kwargs)` and `copy_with(**kwargs)` can set. `None` is
/// "not given, keep what's there", and an empty string is a cleared component
/// — which is what an explicit Python `None` extracts to.
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
    pub fn extract(kwargs: &Bound<'_, PyDict>) -> PyResult<Self> {
        let mut components = Self::default();
        for (key, value) in kwargs.iter() {
            match key.extract::<String>()?.as_str() {
                "scheme" => components.scheme = Some(Self::text(&value)?),
                "username" => components.username = Some(Self::text(&value)?),
                "password" => components.password = Some(Self::text(&value)?),
                "host" => components.host = Some(Self::text(&value)?),
                "port" => {
                    components.port = Some(
                        if value.is_none() {
                            None
                        } else {
                            Some(value.extract()?)
                        },
                    );
                }
                "path" => components.path = Some(Self::text(&value)?),
                "query" => components.query = Some(Self::text(&value)?),
                "fragment" => components.fragment = Some(Self::text(&value)?),
                "params" => {
                    components.query = Some(value.extract::<QueryPairs>()?.to_string());
                }
                key => {
                    return Err(
                        PyTypeError::new_err(
                            format!("'{key}' is an invalid keyword argument for URL()"),
                        ),
                    );
                }
            }
        }
        Ok(components)
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
        Err(
            PyTypeError::new_err(
                format!(
                    "URL components must be str, bytes, or None, got {}",
                    value.get_type().name()?
                ),
            ),
        )
    }
}
