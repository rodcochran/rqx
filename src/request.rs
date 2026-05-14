use std::collections::HashMap;
use std::time::Duration;
use url::Url;
use http::{Method, HeaderMap};
use reqwest::{Client, Request};
use pyo3::exceptions::{PyValueError};
use pyo3::prelude::{
    PyAny,
    PyResult,
};
use pyo3::Bound;

use super::exceptions::*;
use super::py_json::{py_to_value};


pub fn build_client_request(
    http_client: &Client,
    // py: Python<'_>, 
    method: &str, 
    url: &str, 
    content: Option<&[u8]>,
    data: Option<HashMap<String, String>>,
    // files: &Bound<'_, PyDict>,
    json: Option<&Bound<'_, PyAny>>,
    params: Option<HashMap<String, String>>,
    headers: Option<HashMap<String, String>>,
    // cookies: &Bound<'_, PyDict>,
    auth: Option<(String, String)>,
    timeout: Option<u64>,
) -> PyResult<Request> {
    let mut builder = http_client
        .request(Method::from_bytes(method.as_bytes()).unwrap(), url);

    let count = [content.is_some(), data.is_some(), json.is_some()]
        .into_iter()
        .filter(|b| *b)
        .count();
    
    if count > 1 {
        return Err(PyValueError::new_err(
            "Only one of content, data, or json may be set",
        ));
    }

    
    if let Some(c) = content {
        builder = builder
            .body(c.to_vec())
    };

    if let Some(d) = data {
        builder = builder
            .form(&d)
    }

    if let Some(j) = json {
        builder = builder
            .json(&py_to_value(
                // py, 
                j
            ))
    };

    if let Some(p) = params {
        builder = builder
            .query(&p)
    };
    
    if let Some(h) = headers {
        builder = builder
            .headers((&h).try_into().expect("valid headers"))
    };

    if let Some(a) = auth {
        builder = builder
            .basic_auth(a.0, Some(a.1))
    }

    if let Some(t) = timeout {
        builder = builder
            .timeout(Duration::from_secs(t))
    };

    let request = builder
        .build()
        .map_err(|e| {
            RqxError::new_err(format!("Failed to build request: {e}"))
        })?;

    return Ok(request)

}


pub fn build_redirect_request(
    http_client: &Client,
    method: Method, 
    url: Url, 
    headers: &HeaderMap,
) -> Request {
    http_client.request(method, url)
        .headers(headers.clone())
        .build()
        .unwrap()
}


/// Pick the request method to use when following an HTTP redirect.
///
/// Per RFC 7231 §6.4, 302 and 303 responses conventionally cause the client
/// to switch to GET on the follow-up request (except for HEAD, which stays
/// HEAD). That's what we implement here.
///
/// Not yet handled — worth revisiting before treating this as complete:
///   - 301 Moved Permanently: historically ambiguous; most clients also
///     downgrade to GET here, and RFC 7231 explicitly permits it. We
///     currently preserve the original method, which is technically
///     spec-compliant but diverges from how browsers / requests / httpx
///     behave in practice.
///   - 307 Temporary Redirect and 308 Permanent Redirect: the RFC-correct
///     behavior is to preserve the original method *and* the request body.
///     We currently preserve the method (because we fall through to the
///     else branch), but the caller rebuilds the redirect request without
///     carrying the body forward, so POST→POST via 307 silently becomes
///     a body-less POST. Fixing that means threading the original body
///     into `build_redirect_request`.
pub fn determine_redirect_method(
    original_method: &Method,
    status_code: u16,
) -> Method {
    if (status_code == 302 || status_code == 303) && original_method != Method::HEAD {
        Method::GET
    } else {
        original_method.to_owned()
    }
}

pub fn determine_redirect_url(
    current_url: &Url,
    location: &str,
) -> PyResult<Url> {
    Ok(current_url.join(location).unwrap())
}