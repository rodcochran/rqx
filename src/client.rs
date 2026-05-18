use pyo3::Bound;
use pyo3::prelude::{Py, PyAny, PyRef, PyResult, Python, pyclass, pymethods};
use std::collections::HashMap;

use crate::http::client::Client;
use crate::py_json::py_to_value;
use crate::stream::{PyAsyncStreamResponse, PyStreamResponse};

use super::exceptions::*;
use super::response::PyResponse;
use super::runtime::RUNTIME;
use super::timeout::PyTimeout;
use super::transport::{AsyncHTTPTransport, HTTPTransport};
use super::url::parse_base_url;

const DEFAULT_TIMEOUT: f64 = 15.0;
const DEFAULT_FOLLOW_REDIRECTS: bool = false;
const DEFAULT_MAX_REDIRECTS: u32 = 20;

// ────────────────────────────────────────────────────────────────────────
// PyClient — synchronous Python-facing client
// ────────────────────────────────────────────────────────────────────────

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct PyClient {
    inner: Client,
}

#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, base_url=None, transport=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<&Bound<'_, PyAny>>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        base_url: Option<&str>,
        transport: Option<PyRef<'_, HTTPTransport>>,
    ) -> PyResult<Self> {
        let timeout_secs = PyTimeout::resolve_request_timeout(timeout, DEFAULT_TIMEOUT)?;
        let follow = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let max_r = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        let parsed_base_url = base_url.map(parse_base_url).transpose()?;

        if transport.is_some() && (verify.is_some() || cert.is_some() || timeout.is_some()) {
            return Err(RqxError::new_err(
                "Cannot specify both transport= and cert=/verify=/timeout=; pass options through one or the other".to_string(),
            ));
        }

        let transport_inner = match transport {
            Some(t) => t.inner.clone(),
            None => HTTPTransport::new(verify, cert, timeout)?.inner,
        };

        Ok(Self {
            inner: Client::new(
                transport_inner,
                timeout_secs,
                follow,
                max_r,
                parsed_base_url,
            ),
        })
    }

    #[getter]
    fn base_url(&self) -> Option<String> {
        self.inner.base_url()
    }

    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.inner.cookies_snapshot()
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn request(
        &self,
        py: Python<'_>,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(py_to_value);
        let timeout_f64 = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner.request(
                method,
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                follow_redirects,
                timeout_f64,
            ),
        )
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner
                .get(url, params, headers, auth, follow_redirects, t),
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner
                .options(url, params, headers, auth, follow_redirects, t),
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner
                .head(url, params, headers, auth, follow_redirects, t),
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner
                .delete(url, params, headers, auth, follow_redirects, t),
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner.post(
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                follow_redirects,
                t,
            ),
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner.put(
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                follow_redirects,
                t,
            ),
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        block_on_inner(
            py,
            self.inner.patch(
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn stream(
        &self,
        py: Python<'_>,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyStreamResponse> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let (response, elapsed) = block_on_inner(
            py,
            self.inner.stream(
                method,
                url,
                content,
                data,
                json_value,
                params,
                headers,
                auth,
                follow_redirects,
                t,
            ),
        )?;
        let mut resp = PyStreamResponse::from_response(response)?;
        resp.elapsed = elapsed;
        Ok(resp)
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
        // No-op exit since reqwest client manages an Arc internally.
    }
}

// ────────────────────────────────────────────────────────────────────────
// PyAsyncClient — async Python-facing client
// ────────────────────────────────────────────────────────────────────────

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct PyAsyncClient {
    inner: Client,
}

#[pymethods]
impl PyAsyncClient {
    #[new]
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, base_url=None, transport=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<&Bound<'_, PyAny>>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        base_url: Option<&str>,
        transport: Option<PyRef<'_, AsyncHTTPTransport>>,
    ) -> PyResult<Self> {
        let timeout_secs = PyTimeout::resolve_request_timeout(timeout, DEFAULT_TIMEOUT)?;
        let follow = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let max_r = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        let parsed_base_url = base_url.map(parse_base_url).transpose()?;

        if transport.is_some() && (verify.is_some() || cert.is_some() || timeout.is_some()) {
            return Err(RqxError::new_err(
                "Cannot specify both transport= and cert=/verify=/timeout=; pass options through one or the other".to_string(),
            ));
        }

        let transport_inner = match transport {
            Some(t) => t.inner.clone(),
            None => AsyncHTTPTransport::new(verify, cert, timeout)?.inner,
        };

        Ok(Self {
            inner: Client::new(
                transport_inner,
                timeout_secs,
                follow,
                max_r,
                parsed_base_url,
            ),
        })
    }

    #[getter]
    fn base_url(&self) -> Option<String> {
        self.inner.base_url()
    }

    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.inner.cookies_snapshot()
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn request<'a>(
        &self,
        py: Python<'a>,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let method = method.to_string();
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .request(
                    &method,
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    follow_redirects,
                    t,
                )
                .await
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .get(&url, params, headers, auth, follow_redirects, t)
                .await
        })
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .options(&url, params, headers, auth, follow_redirects, t)
                .await
        })
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .head(&url, params, headers, auth, follow_redirects, t)
                .await
        })
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .delete(&url, params, headers, auth, follow_redirects, t)
                .await
        })
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .post(
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    follow_redirects,
                    t,
                )
                .await
        })
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .put(
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    follow_redirects,
                    t,
                )
                .await
        })
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
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .patch(
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    follow_redirects,
                    t,
                )
                .await
        })
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, follow_redirects=None, timeout=None))]
    fn stream<'a>(
        &self,
        py: Python<'a>,
        method: &str,
        url: &str,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<&Bound<'_, PyAny>>,
        params: Option<HashMap<String, String>>,
        headers: Option<HashMap<String, String>>,
        auth: Option<(String, String)>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(py_to_value);
        let t = PyTimeout::resolve_request_timeout(timeout, self.inner.timeout_secs())?;
        let method = method.to_string();
        let url = url.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let (response, elapsed) = inner
                .stream(
                    &method,
                    &url,
                    content.as_deref(),
                    data,
                    json_value,
                    params,
                    headers,
                    auth,
                    follow_redirects,
                    t,
                )
                .await?;
            let mut resp = PyAsyncStreamResponse::from_response(response)?;
            resp.elapsed = elapsed;
            Ok(resp)
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
}

// ────────────────────────────────────────────────────────────────────────
// Shared sync helper: detach GIL, enter runtime, block on async future.
// ────────────────────────────────────────────────────────────────────────

fn block_on_inner<F, T>(py: Python<'_>, fut: F) -> PyResult<T>
where
    F: std::future::Future<Output = PyResult<T>> + Send,
    T: Send,
{
    py.detach(|| {
        RUNTIME
            .get()
            .ok_or_else(|| RqxError::new_err("runtime not initialized"))?
            .block_on(fut)
    })
}
