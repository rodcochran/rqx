from pathlib import Path

import pytest
import rqx

# This gets the directory containing the script
script_dir = Path(__file__).resolve().parent


def test_mtls_basic(mtls_server):
    transport = rqx.HTTPTransport(
        cert=f"{script_dir}/ssl/certs/client-combined.pem",
        verify=f"{script_dir}/ssl/certs/ca-cert.pem",
    )
    client = rqx.Client(transport=transport)
    resp = client.get(url=f"{mtls_server}/mtls")
    assert resp.status_code == 200


# def test_mtls_with_bytes(mtls_server): ...


# def test_mtls_invalid_pem(mtls_server): ...


# @pytest.mark.asyncio
# async def test_mtls_basic_async(mtls_server): ...


# @pytest.mark.asyncio
# async def test_mtls_with_bytes_async(mtls_server): ...


# @pytest.mark.asyncio
# async def test_mtls_invalid_pem_async(mtls_server): ...
