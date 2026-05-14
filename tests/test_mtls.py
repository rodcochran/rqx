from pathlib import Path

import pytest
import rqx

# This gets the directory containing the pems
script_dir = Path(__file__).resolve().parent


# ================================
# Transport-level mTLS tests
# ================================


def test_mtls_basic(mtls_server):
    transport = rqx.HTTPTransport(
        cert=f"{script_dir}/ssl/certs/client-combined.pem",
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.Client(transport=transport)
    resp = client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


def test_mtls_with_bytes(mtls_server):
    with open(f"{script_dir}/ssl/certs/client-combined.pem", "rb") as pem_file:
        pem_bytes = pem_file.read()

    transport = rqx.HTTPTransport(
        cert=pem_bytes,
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.Client(transport=transport)
    resp = client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


def test_mtls_with_tuple(mtls_server):
    transport = rqx.HTTPTransport(
        cert=(
            f"{script_dir}/ssl/certs/client-cert.pem",
            f"{script_dir}/ssl/certs/client-key.pem",
        ),
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.Client(transport=transport)
    resp = client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


def test_mtls_invalid_pem():
    with pytest.raises(rqx.RqxError):
        rqx.HTTPTransport(cert=b"not actually pem at all")


def test_mtls_invalid_cert_type():
    with pytest.raises(rqx.RqxError, match="cert must be"):
        rqx.HTTPTransport(cert=42)  # not a string, bytes, or tuple


def test_mtls_rejects_server_cert_as_client_cert(mtls_server):
    transport = rqx.HTTPTransport(
        cert=(
            f"{script_dir}/ssl/certs/server-cert.pem",
            f"{script_dir}/ssl/certs/server-key.pem",
        ),
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.Client(transport=transport)
    with pytest.raises(rqx.RqxError):
        client.get(url=f"{mtls_server}/mtls")


def test_mtls_no_cert_rejected(mtls_server):
    transport = rqx.HTTPTransport(verify=f"{script_dir}/ssl/certs/ca-cert.pem")
    client = rqx.Client(transport=transport)
    with pytest.raises(rqx.RqxError):
        client.get(url=f"{mtls_server}/mtls")


# ================================
# Async Transport-level mTLS tests
# ================================


@pytest.mark.asyncio
async def test_mtls_basic_async(mtls_server):
    transport = rqx.AsyncHTTPTransport(
        cert=f"{script_dir}/ssl/certs/client-combined.pem",
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_mtls_with_bytes_async(mtls_server):
    with open(f"{script_dir}/ssl/certs/client-combined.pem", "rb") as pem_file:
        pem_bytes = pem_file.read()

    transport = rqx.AsyncHTTPTransport(
        cert=pem_bytes,
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_mtls_with_tuple_async(mtls_server):
    transport = rqx.AsyncHTTPTransport(
        cert=(
            f"{script_dir}/ssl/certs/client-cert.pem",
            f"{script_dir}/ssl/certs/client-key.pem",
        ),
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.AsyncClient(transport=transport)
    resp = await client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_mtls_invalid_pem_async():
    with pytest.raises(rqx.RqxError):
        rqx.AsyncHTTPTransport(cert=b"not actually pem at all")


@pytest.mark.asyncio
async def test_mtls_invalid_cert_type_async():
    with pytest.raises(rqx.RqxError, match="cert must be"):
        rqx.AsyncHTTPTransport(cert=42)


@pytest.mark.asyncio
async def test_mtls_rejects_server_cert_as_client_cert_async(mtls_server):
    transport = rqx.AsyncHTTPTransport(
        cert=(
            f"{script_dir}/ssl/certs/server-cert.pem",
            f"{script_dir}/ssl/certs/server-key.pem",
        ),
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.AsyncClient(transport=transport)
    with pytest.raises(rqx.RqxError):
        await client.get(url=f"{mtls_server}/mtls")


@pytest.mark.asyncio
async def test_mtls_no_cert_rejected_async(mtls_server):
    transport = rqx.AsyncHTTPTransport(verify=f"{script_dir}/ssl/certs/ca-cert.pem")
    client = rqx.AsyncClient(transport=transport)
    with pytest.raises(rqx.RqxError):
        await client.get(url=f"{mtls_server}/mtls")


# ================================
# Client-level mTLS tests
# ================================


def test_dual_specification_raises_error():

    transport = rqx.HTTPTransport(
        cert=f"{script_dir}/ssl/certs/client-combined.pem",
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    with pytest.raises(rqx.RqxError):
        rqx.Client(
            transport=transport,
            cert=f"{script_dir}/ssl/certs/client-combined.pem",
            verify=f"{script_dir}/ssl/certs/ca-cert.pem",
        )


def test_client_mtls_basic(mtls_server):
    client = rqx.Client(
        cert=f"{script_dir}/ssl/certs/client-combined.pem",
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    resp = client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


def test_client_mtls_with_bytes(mtls_server):
    with open(f"{script_dir}/ssl/certs/client-combined.pem", "rb") as pem_file:
        pem_bytes = pem_file.read()

    client = rqx.Client(
        cert=pem_bytes,
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    resp = client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


def test_client_mtls_with_tuple(mtls_server):
    client = rqx.Client(
        cert=(
            f"{script_dir}/ssl/certs/client-cert.pem",
            f"{script_dir}/ssl/certs/client-key.pem",
        ),
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    resp = client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


def test_client_mtls_invalid_pem():
    with pytest.raises(rqx.RqxError):
        rqx.Client(cert=b"not actually pem at all")


def test_client_mtls_invalid_cert_type():
    with pytest.raises(rqx.RqxError, match="cert must be"):
        rqx.Client(cert=42)  # not a string, bytes, or tuple


def test_client_mtls_rejects_server_cert_as_client_cert(mtls_server):
    client = rqx.Client(
        cert=(
            f"{script_dir}/ssl/certs/server-cert.pem",
            f"{script_dir}/ssl/certs/server-key.pem",
        ),
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    with pytest.raises(rqx.RqxError):
        client.get(url=f"{mtls_server}/mtls")


def test_client_mtls_no_cert_rejected(mtls_server):
    client = rqx.Client(verify=f"{script_dir}/ssl/certs/ca-cert.pem")
    with pytest.raises(rqx.RqxError):
        client.get(url=f"{mtls_server}/mtls")


# ================================
# Async Client-level mTLS tests
# ================================


@pytest.mark.asyncio
async def test_dual_specification_raises_error_async():
    transport = rqx.AsyncHTTPTransport(
        cert=f"{script_dir}/ssl/certs/client-combined.pem",
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    with pytest.raises(rqx.RqxError):
        rqx.AsyncClient(
            transport=transport,
            cert=f"{script_dir}/ssl/certs/client-combined.pem",
            verify=f"{script_dir}/ssl/certs/ca-cert.pem",
        )


@pytest.mark.asyncio
async def test_client_mtls_basic_async(mtls_server):
    client = rqx.AsyncClient(
        cert=f"{script_dir}/ssl/certs/client-combined.pem",
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    resp = await client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_client_mtls_with_bytes_async(mtls_server):
    with open(f"{script_dir}/ssl/certs/client-combined.pem", "rb") as pem_file:
        pem_bytes = pem_file.read()

    client = rqx.AsyncClient(
        cert=pem_bytes,
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    resp = await client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_client_mtls_with_tuple_async(mtls_server):
    client = rqx.AsyncClient(
        cert=(
            f"{script_dir}/ssl/certs/client-cert.pem",
            f"{script_dir}/ssl/certs/client-key.pem",
        ),
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    resp = await client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


@pytest.mark.asyncio
async def test_client_mtls_invalid_pem_async():
    with pytest.raises(rqx.RqxError):
        rqx.AsyncClient(cert=b"not actually pem at all")


@pytest.mark.asyncio
async def test_client_mtls_invalid_cert_type_async():
    with pytest.raises(rqx.RqxError, match="cert must be"):
        rqx.AsyncClient(cert=42)


@pytest.mark.asyncio
async def test_client_mtls_rejects_server_cert_as_client_cert_async(mtls_server):
    client = rqx.AsyncClient(
        cert=(
            f"{script_dir}/ssl/certs/server-cert.pem",
            f"{script_dir}/ssl/certs/server-key.pem",
        ),
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    with pytest.raises(rqx.RqxError):
        await client.get(url=f"{mtls_server}/mtls")


@pytest.mark.asyncio
async def test_client_mtls_no_cert_rejected_async(mtls_server):
    client = rqx.AsyncClient(verify=f"{script_dir}/ssl/certs/ca-cert.pem")
    with pytest.raises(rqx.RqxError):
        await client.get(url=f"{mtls_server}/mtls")
