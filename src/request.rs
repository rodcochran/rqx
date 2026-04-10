use std::collections::HashMap;
use std::time::Duration;
use url::Url;
use http::{Method, HeaderMap};
use reqwest::{Client, Request};
use pyo3::exceptions::{PyValueError};
use pyo3::prelude::{PyAny, PyResult, Python};
use pyo3::Bound;

use super::exceptions::*;
use super::py_json::{py_to_value};
use super::response::PyResponse;


pub fn build_client_request(
    http_client: &Client,
    py: Python<'_>, 
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
            .json(&py_to_value(py, j))
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
            ReqxError::new_err(format!("Failed to build request: {e}"))
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


pub fn determine_redirect_method(
    original_method: &Method,
    response: &PyResponse
) -> Method {
    // Get new Method
    if response.status_code == 303 && original_method != Method::HEAD {
        return Method::GET;
    }
    else if response.status_code == 302 && original_method != Method::HEAD {
        return Method::GET;
    }
    else {
        return original_method.to_owned();
    }
}

pub fn determine_redirect_url(
    current_url: &Url,
    response: &PyResponse
) -> PyResult<Url> {
    let location = response
        .headers
        .get("location")
        .unwrap();
    Ok(current_url.join(location.as_str()).unwrap())
}