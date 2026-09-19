use pyo3::Bound;
use pyo3::prelude::{Py, PyAny, PyRef, PyResult, Python, pyclass, pymethods};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex as TokioMutex;

use rqx_core::client::Client;

use crate::exceptions::*;
use crate::py_json::JsonBody;
use crate::query_params::QueryPairs;
use crate::request::{RequestBody, RequestSpec};
use crate::request_headers::RequestHeaders;
use crate::response::{PendingResponse, PyResponse};
use crate::retry::DEFAULT_RAISE_ON_REDIRECT;
use crate::runtime::RUNTIME;
use crate::stream_context::{PyAsyncStreamContext, PyStreamContext};
use crate::timeout::PyTimeout;
use crate::transport::{AsyncHTTPTransport, HTTPTransport, Transport};
use crate::url::{BaseUrl, PyURL, RequestUrl};

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
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, base_url=None, auth_bearer=None, transport=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<&Bound<'_, PyAny>>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        base_url: Option<RequestUrl>,
        auth_bearer: Option<String>,
        transport: Option<PyRef<'_, HTTPTransport>>,
    ) -> PyResult<Self> {
        let timeout_secs = PyTimeout::resolve_request_timeout(
            timeout,
            DEFAULT_TIMEOUT,
        )?;
        let follow = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let max_r = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        let parsed_base_url = base_url
            .map(|url| BaseUrl::parse(url.as_str()))
            .transpose()?;

        if transport.is_some() && (verify.is_some() || cert.is_some() || timeout.is_some()) {
            return Err(RqxError::new_err(
                "Cannot specify both transport= and cert=/verify=/timeout=; pass options through one or the other".to_string(),
            ));
        }

        let transport_inner = match transport {
            Some(t) => t.inner.clone(),
            None => {
                HTTPTransport::new(
                    verify, cert, timeout,
                )?
                .inner
            }
        };

        Ok(
            Self {
                inner: Client::new(
                    transport_inner,
                    timeout_secs,
                    follow,
                    max_r,
                    parsed_base_url,
                    auth_bearer,
                ),
            },
        )
    }

    #[getter]
    fn base_url(&self) -> Option<PyURL> {
        self.inner.base_url()
    }

    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.inner.cookies_snapshot()
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn request(
        &self,
        py: Python<'_>,
        method: &str,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(JsonBody::into_value);
        let timeout_f64 = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
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
                auth_bearer,
                follow_redirects,
                timeout_f64,
            ),
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn get(
        &self,
        py: Python<'_>,
        url: RequestUrl,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        block_on_inner(
            py,
            self.inner.get(
                url,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn options(
        &self,
        py: Python<'_>,
        url: RequestUrl,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        block_on_inner(
            py,
            self.inner.options(
                url,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn head(
        &self,
        py: Python<'_>,
        url: RequestUrl,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        block_on_inner(
            py,
            self.inner.head(
                url,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn delete(
        &self,
        py: Python<'_>,
        url: RequestUrl,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        block_on_inner(
            py,
            self.inner.delete(
                url,
                params,
                headers,
                auth,
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn post(
        &self,
        py: Python<'_>,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
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
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn put(
        &self,
        py: Python<'_>,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
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
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn patch(
        &self,
        py: Python<'_>,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyResponse> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
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
                auth_bearer,
                follow_redirects,
                t,
            ),
        )
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn stream(
        &self,
        method: &str,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyStreamContext> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let request = self.inner.build(
            method,
            url,
            content,
            data,
            json_value,
            params,
            headers,
            auth,
            auth_bearer,
            t,
        )?;
        Ok(
            PyStreamContext::new(
                self.inner.clone(),
                request,
                follow_redirects,
            ),
        )
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
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, base_url=None, auth_bearer=None, transport=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<&Bound<'_, PyAny>>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        base_url: Option<RequestUrl>,
        auth_bearer: Option<String>,
        transport: Option<PyRef<'_, AsyncHTTPTransport>>,
    ) -> PyResult<Self> {
        let timeout_secs = PyTimeout::resolve_request_timeout(
            timeout,
            DEFAULT_TIMEOUT,
        )?;
        let follow = follow_redirects.unwrap_or(DEFAULT_FOLLOW_REDIRECTS);
        let max_r = max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        let parsed_base_url = base_url
            .map(|url| BaseUrl::parse(url.as_str()))
            .transpose()?;

        if transport.is_some() && (verify.is_some() || cert.is_some() || timeout.is_some()) {
            return Err(RqxError::new_err(
                "Cannot specify both transport= and cert=/verify=/timeout=; pass options through one or the other".to_string(),
            ));
        }

        let transport_inner = match transport {
            Some(t) => t.inner.clone(),
            None => {
                AsyncHTTPTransport::new(
                    verify, cert, timeout,
                )?
                .inner
            }
        };

        Ok(
            Self {
                inner: Client::new(
                    transport_inner,
                    timeout_secs,
                    follow,
                    max_r,
                    parsed_base_url,
                    auth_bearer,
                ),
            },
        )
    }

    #[getter]
    fn base_url(&self) -> Option<PyURL> {
        self.inner.base_url()
    }

    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.inner.cookies_snapshot()
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn request<'a>(
        &self,
        py: Python<'a>,
        method: &str,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let method = method.to_string();
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        RUNTIME.future_into_py(
            py,
            async move {
                inner
                    .request(
                        &method,
                        url,
                        content.as_deref(),
                        data,
                        json_value,
                        params,
                        headers,
                        auth,
                        auth_bearer,
                        follow_redirects,
                        t,
                    )
                    .await
            },
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn get<'a>(
        &self,
        py: Python<'a>,
        url: RequestUrl,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let inner = self.inner.clone();
        RUNTIME.future_into_py(
            py,
            async move {
                inner
                    .get(
                        url,
                        params,
                        headers,
                        auth,
                        auth_bearer,
                        follow_redirects,
                        t,
                    )
                    .await
            },
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn options<'a>(
        &self,
        py: Python<'a>,
        url: RequestUrl,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let inner = self.inner.clone();
        RUNTIME.future_into_py(
            py,
            async move {
                inner
                    .options(
                        url,
                        params,
                        headers,
                        auth,
                        auth_bearer,
                        follow_redirects,
                        t,
                    )
                    .await
            },
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn head<'a>(
        &self,
        py: Python<'a>,
        url: RequestUrl,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let inner = self.inner.clone();
        RUNTIME.future_into_py(
            py,
            async move {
                inner
                    .head(
                        url,
                        params,
                        headers,
                        auth,
                        auth_bearer,
                        follow_redirects,
                        t,
                    )
                    .await
            },
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn delete<'a>(
        &self,
        py: Python<'a>,
        url: RequestUrl,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let inner = self.inner.clone();
        RUNTIME.future_into_py(
            py,
            async move {
                inner
                    .delete(
                        url,
                        params,
                        headers,
                        auth,
                        auth_bearer,
                        follow_redirects,
                        t,
                    )
                    .await
            },
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn post<'a>(
        &self,
        py: Python<'a>,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        RUNTIME.future_into_py(
            py,
            async move {
                inner
                    .post(
                        url,
                        content.as_deref(),
                        data,
                        json_value,
                        params,
                        headers,
                        auth,
                        auth_bearer,
                        follow_redirects,
                        t,
                    )
                    .await
            },
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn put<'a>(
        &self,
        py: Python<'a>,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        RUNTIME.future_into_py(
            py,
            async move {
                inner
                    .put(
                        url,
                        content.as_deref(),
                        data,
                        json_value,
                        params,
                        headers,
                        auth,
                        auth_bearer,
                        follow_redirects,
                        t,
                    )
                    .await
            },
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn patch<'a>(
        &self,
        py: Python<'a>,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let content = content.map(<[u8]>::to_vec);
        let inner = self.inner.clone();
        RUNTIME.future_into_py(
            py,
            async move {
                inner
                    .patch(
                        url,
                        content.as_deref(),
                        data,
                        json_value,
                        params,
                        headers,
                        auth,
                        auth_bearer,
                        follow_redirects,
                        t,
                    )
                    .await
            },
        )
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn stream(
        &self,
        method: &str,
        url: RequestUrl,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<QueryPairs>,
        headers: Option<RequestHeaders>,
        auth: Option<(
            String,
            String,
        )>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyAsyncStreamContext> {
        let json_value = json.map(JsonBody::into_value);
        let t = PyTimeout::resolve_request_timeout(
            timeout,
            self.inner.timeout_secs(),
        )?;
        let request = self.inner.build(
            method,
            url,
            content,
            data,
            json_value,
            params,
            headers,
            auth,
            auth_bearer,
            t,
        )?;
        Ok(
            PyAsyncStreamContext::new(
                self.inner.clone(),
                request,
                follow_redirects,
            ),
        )
    }

    fn __aenter__<'py>(slf: Py<Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        RUNTIME.future_into_py(
            py,
            async move { Ok(slf) },
        )
    }

    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        RUNTIME.future_into_py(
            py,
            async move { Ok(false) },
        )
    }
}

// ────────────────────────────────────────────────────────────────────────
// Shared sync helper: detach GIL, enter runtime, block on async future.
// ────────────────────────────────────────────────────────────────────────

pub(crate) fn block_on_inner<F, T>(py: Python<'_>, fut: F) -> PyResult<T>
where
    F: std::future::Future<Output = PyResult<T>> + Send,
    T: Send,
{
    py.detach(|| RUNTIME.block_on(fut))
        .and_then(|result| result)
}
