use http::{HeaderMap, Method};
use pyo3::Bound;
use pyo3::prelude::{Py, PyAny, PyRef, PyResult, Python, pyclass, pymethods};
use reqwest::Request;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;
use url::Url;

use crate::stream::{PyAsyncStreamResponse, PyStreamResponse};

use super::exceptions::*;
use super::request::{
    build_client_request, build_redirect_request, determine_redirect_method, determine_redirect_url,
};
use super::response::PyResponse;
use super::transport::{AsyncHTTPTransport, HTTPTransport};

const DEFAULT_TIMEOUT: u64 = 15;
const DEFAULT_FOLLOW_REDIRECTS: bool = false;
const DEFAULT_MAX_REDIRECTS: u32 = 20;

#[pyclass]
pub struct PyClient {
    transport: HTTPTransport,
    timeout_secs: u64,
    follow_redirects: bool,
    max_redirects: u32,
    cookies: Mutex<HashMap<String, String>>,
}

#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, transport=None, ))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<u64>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        // retries: Option<PyRef<'_, PyRetry>>,
        transport: Option<PyRef<'_, HTTPTransport>>,
    ) -> PyResult<Self> {
        let timeout_secs = timeout.unwrap_or(DEFAULT_TIMEOUT);
        let client_level_follow_redirects = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let client_level_max_redirects = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);

        if transport.is_some() & (verify.is_some() | cert.is_some()) {
            return Err(TooManyRedirects::new_err(
                "Cannot specify both transport= and cert=/verify=; pass options through one or the other ".to_string(),
            ));
        }

        let _transport = match transport {
            Some(t) => t.clone(),
            None => HTTPTransport::new(verify, cert)?,
        };

        Ok(Self {
            transport: _transport,
            timeout_secs: timeout_secs,
            follow_redirects: client_level_follow_redirects,
            max_redirects: client_level_max_redirects,
            cookies: Mutex::new(HashMap::<String, String>::new()),
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
            self.transport.client(),
            // py,
            method,
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            // setting default timeout from top level
            Some(timeout.unwrap_or(self.timeout_secs)),
        )?;

        let _follow_redirects = match follow_redirects {
            Some(fr) => fr,
            None => self.follow_redirects,
        };

        let mut resp = if _follow_redirects {
            self.send_handling_redirects(py, request)?
        } else {
            self.transport.handle_request(py, request)?
        };

        // collecting cookies here - though we should also accumulate them on redirects...
        self.update_cookies(&resp);

        let end_time = std::time::Instant::now();
        let total = end_time - start_time;
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
        self.request(
            py,
            "GET",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
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
        self.request(
            py,
            "OPTIONS",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
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
        self.request(
            py,
            "HEAD",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
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
        self.request(
            py,
            "POST",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
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
        self.request(
            py,
            "PUT",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
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
        self.request(
            py,
            "PATCH",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
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
        self.request(
            py,
            "DELETE",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
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
    fn stream(
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
    ) -> PyResult<PyStreamResponse> {
        let start_time = std::time::Instant::now();

        let request = build_client_request(
            self.transport.client(),
            // py,
            method,
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            // setting default timeout from top level
            Some(timeout.unwrap_or(self.timeout_secs)),
        )?;

        let _follow_redirects = match follow_redirects {
            Some(fr) => fr,
            None => self.follow_redirects,
        };

        let mut resp = if _follow_redirects {
            todo!("Implement redirect handling for stream responses.")
            // self.send_handling_redirects(py, request)?
        } else {
            // self.transport.handle_request(py, request)?
            PyStreamResponse::from_response(self.transport.send_raw(py, request)?)?
        };

        // collecting cookies here - though we should also accumulate them on redirects...
        self.update_cookies_from_stream(&resp);

        let end_time = std::time::Instant::now();
        let total = end_time - start_time;
        resp.elapsed = total.as_secs_f64();
        return Ok(resp);
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
    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.cookies.lock().unwrap().clone()
    }
}

///    Internal functions for PyClient.
///
///    This impl of PyClient is for defining functions that are not to be wrapped in #pymethods,
///    and therefore not exposed to Python.
///
impl PyClient {
    fn send_handling_redirects(&self, py: Python<'_>, request: Request) -> PyResult<PyResponse> {
        let original_method = request.method().clone();
        let original_url = request.url().clone();
        let original_headers = request.headers().clone();
        let mut current_response = self.transport.handle_request(py, request).unwrap();
        // Capture any cookies from the first response before we follow the
        // redirect chain. Each intermediate hop can set cookies we'd
        // otherwise lose, since the non-redirect return path also calls
        // update_cookies once and would only see the final response.
        self.update_cookies(&current_response);

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
            self.update_cookies(&current_response);
        }

        if (300..400).contains(&current_response.status_code) {
            return Err(TooManyRedirects::new_err(format!(
                "Exceeded max redirects {}",
                &self.max_redirects
            )));
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
        let new_url = determine_redirect_url(original_url, resp)
            .map_err(|e| RqxError::new_err(format!("Error parsing url from redirect: {e}")))?;

        let new_method = determine_redirect_method(original_method, resp);
        let current_request = build_redirect_request(
            self.transport.client(),
            new_method,
            new_url,
            original_headers,
        );
        let current_response = self.transport.handle_request(py, current_request);
        return current_response;
    }

    fn update_cookies(&self, resp: &PyResponse) {
        // Skip the mutex when the response has no cookies — the common case.
        // Most responses don't carry Set-Cookie, and taking the lock per
        // request adds a serialization point on the return path under load.
        if resp.cookies.is_empty() {
            return;
        }
        let mut cookies = self.cookies.lock().unwrap();
        cookies.extend(resp.cookies.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    fn update_cookies_from_stream(&self, resp: &PyStreamResponse) {
        if resp.cookies.is_empty() {
            return;
        }
        let mut cookies = self.cookies.lock().unwrap();
        cookies.extend(resp.cookies.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
}

#[pyclass]
pub struct PyAsyncClient {
    // http_client: Client,
    transport: AsyncHTTPTransport,
    timeout_secs: u64,
    follow_redirects: bool,
    max_redirects: u32,
    cookies: Arc<TokioMutex<HashMap<String, String>>>,
}

#[pymethods]
impl PyAsyncClient {
    #[new]
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, transport=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<u64>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        transport: Option<PyRef<'_, AsyncHTTPTransport>>,
    ) -> PyResult<Self> {
        let timeout_secs = timeout.unwrap_or(DEFAULT_TIMEOUT);
        let client_level_follow_redirects = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let client_level_max_redirects = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);

        if transport.is_some() & (verify.is_some() | cert.is_some()) {
            return Err(TooManyRedirects::new_err(
                "Cannot specify both transport= and cert=/verify=; pass options through one or the other ".to_string(),
            ));
        }

        let _transport = match transport {
            Some(t) => t.clone(),
            None => AsyncHTTPTransport::new(verify, cert)?,
        };
        Ok(Self {
            transport: _transport,
            timeout_secs: timeout_secs,
            follow_redirects: client_level_follow_redirects,
            max_redirects: client_level_max_redirects,
            cookies: Arc::new(TokioMutex::new(HashMap::<String, String>::new())),
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
    ) -> PyResult<Bound<'a, PyAny>> {
        let start_time = std::time::Instant::now();

        let request = build_client_request(
            self.transport.client(),
            // py,
            method,
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            // Fall back to the client-level default when no per-request timeout
            // was provided, matching the sync client's behavior.
            Some(timeout.unwrap_or(self.timeout_secs)),
        )?;

        let _follow_redirects = match follow_redirects {
            Some(fr) => fr,
            None => self.follow_redirects,
        };

        let transport = self.transport.clone();
        let max_redirects = self.max_redirects;

        let cookies = Arc::clone(&self.cookies);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut resp = if _follow_redirects {
                Self::send_handling_redirects(
                    transport,
                    request,
                    max_redirects,
                    Arc::clone(&cookies),
                )
                .await?
            } else {
                AsyncHTTPTransport::handle_request(transport, request).await?
            };
            Self::accumulate_cookies(&cookies, &resp.cookies).await;
            let end_time = std::time::Instant::now();
            let total = end_time - start_time;
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
    ) -> PyResult<Bound<'a, PyAny>> {
        self.request(
            py,
            "GET",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn options<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.request(
            py,
            "OPTIONS",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn head<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.request(
            py,
            "HEAD",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn post<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.request(
            py,
            "POST",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn put<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.request(
            py,
            "PUT",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn patch<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.request(
            py,
            "PATCH",
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn delete<'a>(
        &self,
        py: Python<'a>,
        url: &str,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<u64>,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.request(
            py,
            "DELETE",
            url,
            None,
            None,
            None,
            params,
            headers,
            auth,
            follow_redirects,
            timeout,
        )
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
    fn stream<'a>(
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
    ) -> PyResult<Bound<'a, PyAny>> {
        let start_time = std::time::Instant::now();

        let request = build_client_request(
            self.transport.client(),
            // py,
            method,
            url,
            content,
            data,
            json,
            params,
            headers,
            auth,
            // Fall back to the client-level default when no per-request timeout
            // was provided, matching the sync client's behavior.
            Some(timeout.unwrap_or(self.timeout_secs)),
        )?;

        let _follow_redirects = match follow_redirects {
            Some(fr) => fr,
            None => self.follow_redirects,
        };

        let transport = self.transport.clone();
        // let max_redirects = self.max_redirects;

        let cookies = Arc::clone(&self.cookies);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut resp = if _follow_redirects {
                // Self::send_handling_redirects(transport, request, max_redirects).await?
                todo!("Implement async redirect handling for stream")
            } else {
                PyAsyncStreamResponse::from_response(
                    AsyncHTTPTransport::send_raw(
                        transport.client(),
                        request,
                        transport.semaphore(),
                    )
                    .await?,
                )?
            };
            Self::accumulate_cookies(&cookies, &resp.cookies).await;
            let end_time = std::time::Instant::now();
            let total = end_time - start_time;
            resp.elapsed = total.as_secs_f64();
            return Ok(resp);
        })
    }

    fn __aenter__<'py>(slf: Py<Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(slf) })
    }

    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(false) })
    }
    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.cookies.blocking_lock().clone()
    }
}

impl PyAsyncClient {
    /// Merge response cookies into the client's Python-visible jar.
    ///
    /// Skips the mutex entirely when the response carries no cookies (the
    /// common case on a successful response), so the jar is only a
    /// serialization point on responses that actually set cookies.
    async fn accumulate_cookies(
        cookies: &Arc<TokioMutex<HashMap<String, String>>>,
        resp_cookies: &HashMap<String, String>,
    ) {
        if resp_cookies.is_empty() {
            return;
        }
        cookies
            .lock()
            .await
            .extend(resp_cookies.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    async fn send_handling_redirects(
        transport: AsyncHTTPTransport,
        request: Request,
        max_redirects: u32,
        cookies: Arc<TokioMutex<HashMap<String, String>>>,
    ) -> PyResult<PyResponse> {
        let original_method = request.method().clone();
        let original_url = request.url().clone();
        let original_headers = request.headers().clone();
        let mut current_response =
            AsyncHTTPTransport::handle_request(transport.clone(), request).await?;
        // Capture any cookies set by the first response before we follow the
        // redirect chain. Without this, Set-Cookie headers from intermediate
        // hops are dropped from the Python-visible client.cookies view.
        Self::accumulate_cookies(&cookies, &current_response.cookies).await;

        for _ in 1..max_redirects {
            if !(300..400).contains(&current_response.status_code) {
                return Ok(current_response);
            }
            current_response = Self::handle_redirect(
                &transport,
                &original_url,
                &original_method,
                &original_headers,
                &current_response,
            )
            .await?;
            Self::accumulate_cookies(&cookies, &current_response.cookies).await;
        }

        if (300..400).contains(&current_response.status_code) {
            return Err(TooManyRedirects::new_err(format!(
                "Exceeded max redirects {}",
                max_redirects
            )));
        }
        Ok(current_response)
    }

    async fn handle_redirect(
        transport: &AsyncHTTPTransport,
        original_url: &Url,
        original_method: &Method,
        original_headers: &HeaderMap,
        resp: &PyResponse,
    ) -> PyResult<PyResponse> {
        let new_url = determine_redirect_url(original_url, resp)
            .map_err(|e| RqxError::new_err(format!("Error parsing url from redirect: {e}")))?;

        let new_method = determine_redirect_method(original_method, resp);
        let current_request =
            build_redirect_request(transport.client(), new_method, new_url, original_headers);
        let current_response =
            AsyncHTTPTransport::handle_request(transport.clone(), current_request).await;
        return current_response;
    }
}
