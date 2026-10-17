import pytest
import respx

from alcedo_sdk.resources.usage import UsageResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def usage(http_client):
    return UsageResource(http_client)


@pytest.mark.asyncio
async def test_get(usage):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/usage/cpu").respond(json=jsend({"percent": 12.5}))
        result = await usage.get("cpu")
        assert result == {"percent": 12.5}


@pytest.mark.asyncio
async def test_callable(usage):
    """client.usage mirrors the node SDK's callable usage resource."""
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/usage/memory").respond(json=jsend({"used_mb": 100}))
        result = await usage("memory")
        assert result == {"used_mb": 100}
