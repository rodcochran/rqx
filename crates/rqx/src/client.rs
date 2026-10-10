use std::collections::HashMap;

use pyo3::Bound;
use pyo3::prelude::{Py, PyAny, PyRef, PyResult, Python, pyclass, pymethods};

use rqx_core::auth::Auth;
use rqx_core::client::{Client, ClientConfig};
use rqx_core::error::RqxCoreError;
use rqx_core::request::Request;
use rqx_core::request_components::body::RequestBody;
use rqx_core::timeout::Timeout;
use rqx_core::transport::{Transport, TransportConfig};
use rqx_core::url::base_url::BaseUrl;

use crate::exceptions::*;
use crate::py_json::JsonBody;
use crate::query_params::RequestQueryParams;
use crate::redirect::PyRedirectPolicy;
use crate::request_headers::RequestHeaders;
use crate::response::PyResponse;
use crate::runtime::RUNTIME;
use crate::stream_context::{PyAsyncStreamContext, PyStreamContext};
use crate::timeout::TimeoutArg;
use crate::transport::TransportArgs;
use crate::transport::{AsyncHTTPTransport, HTTPTransport};
use crate::url::py_url::PyURL;

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
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, base_url=None, auth_bearer=None, transport=None, redirects=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<TimeoutArg<'_>>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        base_url: Option<PyURL>,
        auth_bearer: Option<String>,
        transport: Option<PyRef<'_, HTTPTransport>>,
        redirects: Option<PyRedirectPolicy>,
    ) -> Result<Self, PyRqxError> {
        let parsed_base_url = base_url.map(|url| BaseUrl::new(url.inner)).transpose()?;

        let redirect_policy = PyRedirectPolicy::valid_policy_from_options(
            follow_redirects,
            max_redirects,
            redirects,
        )?;

        let timeout = timeout.map(Timeout::from);
        let auth_config = Auth::new(None, auth_bearer)?;
        let config = ClientConfig::new(
            timeout.clone().unwrap_or_default(),
            redirect_policy,
            parsed_base_url,
            auth_config,
        );

        if transport.is_some() && (verify.is_some() || cert.is_some() || timeout.is_some()) {
            return Err(RqxError::new_err(
                "Cannot specify both transport= and cert=/verify=/timeout=; pass options through one or the other",
            )
            .into());
        }

        let transport_inner = match transport {
            Some(t) => t.inner.clone(),
            None => Transport::new(TransportConfig::try_from(TransportArgs {
                verify,
                cert,
                timeout,
                ..Default::default()
            })?)?,
        };

        Ok(Self {
            inner: Client::new(transport_inner, config),
        })
    }

    #[getter]
    fn base_url(&self) -> Option<PyURL> {
        self.inner.base_url().map(PyURL::from_base_url)
    }

    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.inner.cookies_snapshot()
    }

    #[getter]
    fn redirects(&self) -> PyRedirectPolicy {
        PyRedirectPolicy {
            inner: self.inner.redirects(),
        }
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn request(
        &self,
        py: Python<'_>,
        method: &str,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyResponse, PyRqxError> {
        let json_value = json.map(JsonBody::into_value);
        let timeout_f64 = timeout
            .map(Timeout::from)
            .and_then(|t| t.per_request_total());
        let auth_config = match (&auth, &auth_bearer) {
            (None, None) => None,
            _ => Some(Auth::new(auth, auth_bearer)?),
        };

        let body = RequestBody::new(content, data, json_value)?;

        let request = Request::new(
            method,
            url.inner,
            params.map(|p| p.inner),
            headers.map(|h| h.inner),
            body,
            auth_config,
            timeout_f64,
            follow_redirects,
        )?;
        block_on_inner(py, self.inner.request(request)).map(PyResponse::from)
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn get(
        &self,
        py: Python<'_>,
        url: PyURL,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyResponse, PyRqxError> {
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn options(
        &self,
        py: Python<'_>,
        url: PyURL,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyResponse, PyRqxError> {
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn head(
        &self,
        py: Python<'_>,
        url: PyURL,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyResponse, PyRqxError> {
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn delete(
        &self,
        py: Python<'_>,
        url: PyURL,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyResponse, PyRqxError> {
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn post(
        &self,
        py: Python<'_>,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyResponse, PyRqxError> {
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn put(
        &self,
        py: Python<'_>,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyResponse, PyRqxError> {
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn patch(
        &self,
        py: Python<'_>,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyResponse, PyRqxError> {
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn stream(
        &self,
        method: &str,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyStreamContext, PyRqxError> {
        let json_value = json.map(JsonBody::into_value);
        let t = timeout
            .map(Timeout::from)
            .and_then(|t| t.per_request_total());
        let auth_config = match (&auth, &auth_bearer) {
            (None, None) => None,
            _ => Some(Auth::new(auth, auth_bearer)?),
        };
        let body = RequestBody::new(content, data, json_value)?;

        let request = Request::new(
            method,
            url.inner,
            params.map(|p| p.inner),
            headers.map(|h| h.inner),
            body,
            auth_config,
            t,
            follow_redirects,
        )?;
        Ok(PyStreamContext::new(self.inner.stream(request)?))
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
    #[pyo3(signature = (verify=None, cert=None, timeout=None, follow_redirects=None, max_redirects=None, base_url=None, auth_bearer=None, transport=None, redirects=None))]
    fn __new__(
        verify: Option<&Bound<'_, PyAny>>,
        cert: Option<&Bound<'_, PyAny>>,
        timeout: Option<TimeoutArg<'_>>,
        follow_redirects: Option<bool>,
        max_redirects: Option<u32>,
        base_url: Option<PyURL>,
        auth_bearer: Option<String>,
        transport: Option<PyRef<'_, AsyncHTTPTransport>>,
        redirects: Option<PyRedirectPolicy>,
    ) -> Result<Self, PyRqxError> {
        let parsed_base_url = base_url.map(|url| BaseUrl::new(url.inner)).transpose()?;
        let redirect_policy = PyRedirectPolicy::valid_policy_from_options(
            follow_redirects,
            max_redirects,
            redirects,
        )?;
        let timeout = timeout.map(Timeout::from);
        let auth_config = Auth::new(None, auth_bearer)?;
        let config = ClientConfig::new(
            timeout.clone().unwrap_or_default(),
            redirect_policy,
            parsed_base_url,
            auth_config,
        );

        if transport.is_some() && (verify.is_some() || cert.is_some() || timeout.is_some()) {
            return Err(RqxError::new_err(
                "Cannot specify both transport= and cert=/verify=/timeout=; pass options through one or the other",
            )
            .into());
        }

        let transport_inner = match transport {
            Some(t) => t.inner.clone(),
            None => Transport::new(TransportConfig::try_from(TransportArgs {
                verify,
                cert,
                timeout,
                ..Default::default()
            })?)?,
        };

        Ok(Self {
            inner: Client::new(transport_inner, config),
        })
    }

    #[getter]
    fn base_url(&self) -> Option<PyURL> {
        self.inner.base_url().map(PyURL::from_base_url)
    }

    #[getter]
    fn cookies(&self) -> HashMap<String, String> {
        self.inner.cookies_snapshot()
    }

    #[getter]
    fn redirects(&self) -> PyRedirectPolicy {
        PyRedirectPolicy {
            inner: self.inner.redirects(),
        }
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn request<'a>(
        &self,
        py: Python<'a>,
        method: &str,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let json_value = json.map(JsonBody::into_value);
        let t = timeout
            .map(Timeout::from)
            .and_then(|t| t.per_request_total());

        let inner = self.inner.clone();

        let auth_config = match (&auth, &auth_bearer) {
            (None, None) => None,
            _ => Some(Auth::new(auth, auth_bearer).map_err(PyRqxError::Core)?),
        };

        let body = RequestBody::new(content, data, json_value).map_err(PyRqxError::Core)?;

        let request = Request::new(
            method,
            url.inner,
            params.map(|p| p.inner),
            headers.map(|h| h.inner),
            body,
            auth_config,
            t,
            follow_redirects,
        )
        .map_err(PyRqxError::Core)?;
        RUNTIME.future_into_py(py, async move {
            inner.request(request).await.map(PyResponse::from)
        })
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn get<'a>(
        &self,
        py: Python<'a>,
        url: PyURL,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn options<'a>(
        &self,
        py: Python<'a>,
        url: PyURL,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn head<'a>(
        &self,
        py: Python<'a>,
        url: PyURL,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn delete<'a>(
        &self,
        py: Python<'a>,
        url: PyURL,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn post<'a>(
        &self,
        py: Python<'a>,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn put<'a>(
        &self,
        py: Python<'a>,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn patch<'a>(
        &self,
        py: Python<'a>,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
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
            auth_bearer,
            follow_redirects,
            timeout,
        )
    }

    #[pyo3(signature = (method, url, content=None, data=None, json=None, params=None, headers=None, auth=None, auth_bearer=None, follow_redirects=None, timeout=None))]
    fn stream(
        &self,
        method: &str,
        url: PyURL,
        content: Option<&[u8]>,
        data: Option<HashMap<String, String>>,
        json: Option<JsonBody>,
        params: Option<RequestQueryParams>,
        headers: Option<RequestHeaders>,
        auth: Option<(String, String)>,
        auth_bearer: Option<String>,
        follow_redirects: Option<bool>,
        timeout: Option<TimeoutArg<'_>>,
    ) -> Result<PyAsyncStreamContext, PyRqxError> {
        let json_value = json.map(JsonBody::into_value);
        let t = timeout
            .map(Timeout::from)
            .and_then(|t| t.per_request_total());

        let auth_config = match (&auth, &auth_bearer) {
            (None, None) => None,
            _ => Some(Auth::new(auth, auth_bearer).map_err(PyRqxError::Core)?),
        };

        let body = RequestBody::new(content, data, json_value)?;

        let request = Request::new(
            method,
            url.inner,
            params.map(|p| p.inner),
            headers.map(|h| h.inner),
            body,
            auth_config,
            t,
            follow_redirects,
        )?;

        Ok(PyAsyncStreamContext::new(self.inner.stream(request)?))
    }

    fn __aenter__<'py>(slf: Py<Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        RUNTIME.future_into_py(py, async move { Ok::<_, PyRqxError>(slf) })
    }

    fn __aexit__<'py>(
        &self,
        py: Python<'py>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        RUNTIME.future_into_py(py, async move { Ok::<_, PyRqxError>(false) })
    }
}

// ────────────────────────────────────────────────────────────────────────
// Shared sync helper: detach GIL, enter runtime, block on async future.
// ────────────────────────────────────────────────────────────────────────

pub(crate) fn block_on_inner<F, T>(py: Python<'_>, fut: F) -> Result<T, PyRqxError>
where
    F: std::future::Future<Output = Result<T, RqxCoreError>> + Send,
    T: Send,
{
    Ok(py.detach(|| RUNTIME.block_on(fut))??)
}
