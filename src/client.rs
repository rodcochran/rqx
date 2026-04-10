use std::collections::HashMap;
use std::time::Duration;
use http::{Method, HeaderMap};
use url::Url;
use reqwest::{Client, Request};
use pyo3::prelude::{Py, PyAny, PyRef, PyResult, Python,  pyclass, pymethods};
use pyo3::Bound;

use super::runtime::RUNTIME;
use super::exceptions::*;
use super::response::PyResponse;
use super::request::{
    build_client_request, 
    determine_redirect_url, 
    determine_redirect_method, 
    build_redirect_request
};


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
        
        let request = build_client_request(
            &self.http_client,
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
        PyResponse::from_response(py, response)
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
        let new_url = determine_redirect_url(&original_url, &resp)
            .map_err(|e| {
                ReqxError::new_err(format!("Error parsing url from redirect: {e}"))
            }
        )?;
        
        let new_method = determine_redirect_method(&original_method, &resp);
        let current_request = build_redirect_request(
            &self.http_client,
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

}


#[pyclass]
pub struct PyAsyncClient {
    http_client: Client,
    timeout_secs: u64,
    follow_redirects: bool,
    max_redirects: u32
}

#[pymethods]
impl PyAsyncClient {
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
    fn request<'a>(
        &self, 
        py: Python<'a>,
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
    // ) -> PyResult<PyResponse> {
    ) -> PyResult<Bound<'a,PyAny>>{

        let start_time = std::time::Instant::now();
        
        let request = build_client_request(
            &self.http_client,
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

        let client = self.http_client.clone();
        let fut = if _follow_redirects {
            // Uncomment when implementing it.
            // self.send_handling_redirects(py, request)?
            Self::send_single_request(client, request)
        } else {
            Self::send_single_request(client, request)
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut resp = fut.await?;
            let end_time = std::time::Instant::now();
            let total =  end_time - start_time;
            resp.elapsed = total.as_secs_f64();
            return Ok(resp);
        })

    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn get<'a>(
        &self, 
        py: Python<'a>, 
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<Bound<'a,PyAny>>{
        self.request(py, "GET", url, None, None, None, params, headers, auth, follow_redirects, timeout)
    }

    fn __aenter__<'py>(slf: Py<Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(slf)
        })
    }
    
    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(false)
        })
    }
}

impl PyAsyncClient {
    async fn send_single_request(client: Client, request: Request) -> PyResult<PyResponse> {
        let response = client
            .execute(request)
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    TimeoutException::new_err(format!("request timed out: {e}"))
                } else {
                    ReqxError::new_err(format!("request failed: {e}"))
                }
            }
        )?;
        let resp_future = PyResponse::from_response_async(response);
        resp_future.await
    }
}