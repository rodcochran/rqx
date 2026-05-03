import pytest


def test_mtls_basic(mtls_server): ...


def test_mtls_with_bytes(mtls_server): ...


def test_mtls_invalid_pem(mtls_server): ...


@pytest.mark.asyncio
async def test_mtls_basic_async(mtls_server): ...


@pytest.mark.asyncio
async def test_mtls_with_bytes_async(mtls_server): ...


@pytest.mark.asyncio
async def test_mtls_invalid_pem_async(mtls_server): ...
