import pytest
import reqx


@pytest.mark.asyncio
async def test_context_mangers():

    async with reqx.AsyncClient() as client:
        assert client is not None
