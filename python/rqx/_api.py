"""Module-level convenience functions.

Mirrors httpx's ``_api.py``: each function spins up an ephemeral ``Client``
for a single request and tears it down on exit. Convenient for one-off
scripts; if you're making more than a couple of requests, construct a
``Client`` directly so the connection pool is reused.

Kwarg split mirrors the underlying ``Client`` surface:

- Client-construction kwargs (``verify``, ``cert``, ``timeout``) are passed
  to ``Client(...)``.
- Per-request kwargs (``params``, ``headers``, ``auth``, ``follow_redirects``,
  plus ``content`` / ``data`` / ``json`` for body-bearing verbs) are passed
  to the underlying ``client.<verb>(...)`` call.

Knobs like ``base_url``, ``max_redirects``, and ``transport`` are intentionally
omitted — anyone reaching for those is already constructing a long-lived
``Client``. Same omission as httpx.
"""

from __future__ import annotations

from collections.abc import Iterator, Mapping
from contextlib import contextmanager
from typing import Any

from ._rqx import PyClient, PyResponse, PyStreamResponse, Timeout

__all__ = [
    "delete",
    "get",
    "head",
    "options",
    "patch",
    "post",
    "put",
    "request",
    "stream",
]


# Type aliases — kept loose because users routinely pass raw dicts / numbers.
VerifyTypes = bool | str
CertTypes = str | bytes | tuple[str, str]
TimeoutTypes = float | int | Timeout


def request(
    method: str,
    url: str,
    *,
    content: bytes | None = None,
    data: Mapping[str, str] | None = None,
    json: Any | None = None,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> PyResponse:
    """Send a one-off HTTP request.

    Constructs an ephemeral ``Client``, issues the request, and tears the
    client down on return. For more than a handful of requests against the
    same host, use a ``Client`` directly so the connection pool can be reused.
    """
    with PyClient(verify=verify, cert=cert, timeout=timeout) as client:
        return client.request(
            method,
            url,
            content=content,
            data=data,
            json=json,
            params=params,
            headers=headers,
            auth=auth,
            auth_bearer=auth_bearer,
            follow_redirects=follow_redirects,
            timeout=timeout,
        )


@contextmanager
def stream(
    method: str,
    url: str,
    *,
    content: bytes | None = None,
    data: Mapping[str, str] | None = None,
    json: Any | None = None,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> Iterator[PyStreamResponse]:
    """Stream a one-off HTTP response.

    Use as a context manager:

        with rqx.stream("GET", url) as resp:
            for chunk in resp.iter_bytes():
                ...

    Like ``rqx.request``, this constructs an ephemeral ``Client`` per call.
    Use ``Client.stream`` directly if you're issuing multiple streamed
    requests against the same host.
    """
    with PyClient(verify=verify, cert=cert, timeout=timeout) as client:
        with client.stream(
            method,
            url,
            content=content,
            data=data,
            json=json,
            params=params,
            headers=headers,
            auth=auth,
            auth_bearer=auth_bearer,
            follow_redirects=follow_redirects,
            timeout=timeout,
        ) as response:
            yield response


def get(
    url: str,
    *,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> PyResponse:
    """Send a one-off ``GET`` request. See :func:`request` for kwarg semantics."""
    return request(
        "GET",
        url,
        params=params,
        headers=headers,
        auth=auth,
        auth_bearer=auth_bearer,
        follow_redirects=follow_redirects,
        verify=verify,
        cert=cert,
        timeout=timeout,
    )


def options(
    url: str,
    *,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> PyResponse:
    """Send a one-off ``OPTIONS`` request. See :func:`request` for kwarg semantics."""
    return request(
        "OPTIONS",
        url,
        params=params,
        headers=headers,
        auth=auth,
        auth_bearer=auth_bearer,
        follow_redirects=follow_redirects,
        verify=verify,
        cert=cert,
        timeout=timeout,
    )


def head(
    url: str,
    *,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> PyResponse:
    """Send a one-off ``HEAD`` request. See :func:`request` for kwarg semantics."""
    return request(
        "HEAD",
        url,
        params=params,
        headers=headers,
        auth=auth,
        auth_bearer=auth_bearer,
        follow_redirects=follow_redirects,
        verify=verify,
        cert=cert,
        timeout=timeout,
    )


def post(
    url: str,
    *,
    content: bytes | None = None,
    data: Mapping[str, str] | None = None,
    json: Any | None = None,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> PyResponse:
    """Send a one-off ``POST`` request. See :func:`request` for kwarg semantics."""
    return request(
        "POST",
        url,
        content=content,
        data=data,
        json=json,
        params=params,
        headers=headers,
        auth=auth,
        auth_bearer=auth_bearer,
        follow_redirects=follow_redirects,
        verify=verify,
        cert=cert,
        timeout=timeout,
    )


def put(
    url: str,
    *,
    content: bytes | None = None,
    data: Mapping[str, str] | None = None,
    json: Any | None = None,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> PyResponse:
    """Send a one-off ``PUT`` request. See :func:`request` for kwarg semantics."""
    return request(
        "PUT",
        url,
        content=content,
        data=data,
        json=json,
        params=params,
        headers=headers,
        auth=auth,
        auth_bearer=auth_bearer,
        follow_redirects=follow_redirects,
        verify=verify,
        cert=cert,
        timeout=timeout,
    )


def patch(
    url: str,
    *,
    content: bytes | None = None,
    data: Mapping[str, str] | None = None,
    json: Any | None = None,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> PyResponse:
    """Send a one-off ``PATCH`` request. See :func:`request` for kwarg semantics."""
    return request(
        "PATCH",
        url,
        content=content,
        data=data,
        json=json,
        params=params,
        headers=headers,
        auth=auth,
        auth_bearer=auth_bearer,
        follow_redirects=follow_redirects,
        verify=verify,
        cert=cert,
        timeout=timeout,
    )


def delete(
    url: str,
    *,
    params: Mapping[str, str] | None = None,
    headers: Mapping[str, str] | None = None,
    auth: tuple[str, str] | None = None,
    auth_bearer: str | None = None,
    follow_redirects: bool = False,
    verify: VerifyTypes | None = None,
    cert: CertTypes | None = None,
    timeout: TimeoutTypes | None = None,
) -> PyResponse:
    """Send a one-off ``DELETE`` request. See :func:`request` for kwarg semantics."""
    return request(
        "DELETE",
        url,
        params=params,
        headers=headers,
        auth=auth,
        auth_bearer=auth_bearer,
        follow_redirects=follow_redirects,
        verify=verify,
        cert=cert,
        timeout=timeout,
    )
