use std::collections::HashMap;
use std::time::Duration;
use http::{Method, HeaderMap};
use url::Url;
use reqwest::{Client, Request};
use pyo3::exceptions::{PyValueError};
use pyo3::prelude::{PyAny, PyRef, PyResult, Python,  pyclass, pymethods};
use pyo3::Bound;

use super::runtime::RUNTIME;
use super::exceptions::*;
use super::py_json::{py_to_value};
use super::response::PyResponse;


const DEFAULT_TIMEOUT: u64 = 15;
const DEFAULT_FOLLOW_REDIRECTS: bool = false;
const DEFAULT_MAX_REDIRECTS: u32 = 20;


#[pyclass]
pub struct PyClient {
    http_client: Client,
    timeout_secs: u64,
    follow_redirects: bool,
    max_redirects: u32
}


#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (timeout=None, follow_redirects=None, max_redirects=None))]
    fn __new__(
        timeout: Option<u64>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
    ) -> PyResult<Self> {
        let timeout_secs = timeout.unwrap_or(DEFAULT_TIMEOUT);
        let client_level_follow_redirects = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let client_level_max_redirects = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);

        let http_client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            //.connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            // .gzip(true)
            // .brotli(true)
            .pool_max_idle_per_host(20)
            .build()
            .expect("Failed to build HTTP client");
        Ok(Self {
            http_client: http_client,
            timeout_secs: timeout_secs,
            follow_redirects: client_level_follow_redirects,
            max_redirects: client_level_max_redirects,
        })
    }

    /*
    class Client
        def request(
            self,
            method: str,
            url: URL | str,
            *,
            content: RequestContent | None = None,
            data: RequestData | None = None,
            files: RequestFiles | None = None,
            json: typing.Any | None = None,
            params: QueryParamTypes | None = None,
            headers: HeaderTypes | None = None,
            cookies: CookieTypes | None = None,
            auth: AuthTypes | UseClientDefault | None = USE_CLIENT_DEFAULT,
            follow_redirects: bool | UseClientDefault = USE_CLIENT_DEFAULT,
            timeout: TimeoutTypes | UseClientDefault = USE_CLIENT_DEFAULT,
            extensions: RequestExtensions | None = None,
        ) -> Response:
     */
    #[pyo3(
        signature = (
            method, 
            url, 
            content=None, 
            data=None, 
            // files, 
            json=None, 
            params=None, 
            headers=None, 
            // cookies, 
            auth=None, 
            follow_redirects=None, 
            timeout=None
            // extensions
        )
    )]
    fn request(
        &self, 
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
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
        // extensions: &Bound<'_, PyDict>,
    ) -> PyResult<PyResponse> {

        let start_time = std::time::Instant::now();
        
        let request = self.build_request(
            py,
            method,
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            timeout
        )?;

        let _follow_redirects = match follow_redirects {
            Some(fr) => {fr}
            None => {
                self.follow_redirects
            }
        };

        let mut resp = if _follow_redirects {
            self.send_handling_redirects(py, request)?
        } else {
            self.send_single_request(py, request)?
        };

        let end_time = std::time::Instant::now();
        let total =  end_time - start_time;
        resp.elapsed = total.as_secs_f64();
        return Ok(resp);
    }


    #[pyo3(signature = (url, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn get(
        &self, 
        py: Python<'_>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "GET", url, None, None, None, params, headers, auth, follow_redirects, timeout)
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn options(
        &self, 
        py: Python<'_>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "OPTIONS", url, None, None, None, params, headers, auth, follow_redirects, timeout)
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn head(
        &self, 
        py: Python<'_>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "HEAD", url, None, None, None, params, headers, auth, follow_redirects, timeout)
    }


    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn post(
        &self, 
        py: Python<'_>, 
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "POST", url, content, data, json, params, headers, auth, follow_redirects, timeout)
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn put(
        &self, 
        py: Python<'_>, 
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "PUT", url, content, data, json, params, headers, auth, follow_redirects, timeout)
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn patch(
        &self, 
        py: Python<'_>, 
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "PATCH", url, content, data, json, params, headers, auth, follow_redirects, timeout)
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn delete(
        &self, 
        py: Python<'_>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<PyResponse> {
        self.request(py, "DELETE", url, None, None, None, params, headers, auth, follow_redirects, timeout)
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) {
        // No-op exit since Reqwest client manages an Arc internally.
    }
}

///    Internal functions for PyClient.
/// 
///    This impl of PyClient is for defining functions that are not to be wrapped in #pymethods, 
///    and therefore not exposed to Python.
///

impl PyClient {
    fn build_request(
        &self, 
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
        let mut builder = self.http_client
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


    fn send_single_request(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        let response = py.detach(|| {
            RUNTIME
                .get()
                .ok_or_else(|| ReqxError::new_err("runtime not initialized"))?
                .block_on(async {
                    self.http_client
                        .execute(request)
                        .await
                        .map_err(|e| {
                            if e.is_timeout() {
                                TimeoutException::new_err(format!("request timed out: {e}"))
                            } else {
                                ReqxError::new_err(format!("request failed: {e}"))
                            }
                        })
                })
        })?;

        let status_code = response.status().as_u16();

        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    v.to_str().unwrap_or("<non-utf8>").to_string(),
                )
            })
            .collect::<HashMap<_, _>>();
        
        let url = response.url().as_str().to_owned();

        let content = py
            .detach(|| {
                RUNTIME
                    .get()
                    .ok_or_else(|| ReqxError::new_err("runtime not initialized"))?
                    .block_on(async {
                        response.bytes().await.map_err(|e| {
                            ReqxError::new_err(format!("failed to read body: {e}"))
                        })
                    })
            })?
            .to_vec();

        Ok(PyResponse {
            status_code: status_code,
            headers: headers,
            content: content,
            url: url,
            elapsed: 0.0
        })
    }


    fn send_handling_redirects(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        
        let original_method = request.method().clone();
        let original_url = request.url().clone();
        let original_headers = request.headers().clone();
        let mut current_response = self.send_single_request(py, request).unwrap();

        for _ in 1..self.max_redirects {
            if !(300..400).contains(&current_response.status_code) {
                return Ok(current_response);
            }
            current_response = self.handle_redirect(
                py, 
                &original_url,
                &original_method, 
                &original_headers, 
                &current_response,
            )?;
        }

        if (300..400).contains(&current_response.status_code) {
            return Err(TooManyRedirects::new_err(
                format!("Exceeded max redirects {}", &self.max_redirects)));
        }
        Ok(current_response)
    }

    fn handle_redirect(
        &self,
        py: Python<'_>, 
        original_url: &Url, 
        original_method: &Method,
        original_headers: &HeaderMap,
        resp: &PyResponse,
    ) -> PyResult<PyResponse> {
        let new_url = self.determine_redirect_url(&original_url, &resp)
            .map_err(|e| {
                ReqxError::new_err(format!("Error parsing url from redirect: {e}"))
            }
        )?;
        
        let new_method = self.determine_redirect_method(&original_method, &resp);
        let current_request = self.build_redirect_request(
            new_method,
            new_url,
            &original_headers
        );
        let current_response = self.send_single_request(
            py, 
            current_request
        );
        return current_response;
    }

    fn build_redirect_request(
        &self, 
        method: Method, 
        url: Url, 
        headers: &HeaderMap,
    ) -> Request {
        self.http_client.request(method, url)
            .headers(headers.clone())
            .build()
            .unwrap()
    }

    fn determine_redirect_method(
        &self,
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

    fn determine_redirect_url(
        &self,
        current_url: &Url,
        response: &PyResponse
    ) -> PyResult<Url> {
        let location = response
            .headers
            .get("location")
            .unwrap();
        Ok(current_url.join(location.as_str()).unwrap())
    }
}