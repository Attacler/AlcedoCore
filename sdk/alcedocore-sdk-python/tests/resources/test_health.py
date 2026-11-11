import pytest
import respx

from alcedo_sdk.resources.health import HealthResource
from alcedo_sdk.exceptions import ServerError
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def health(http_client):
    return HealthResource(http_client)


@pytest.mark.asyncio
async def test_health_check(health):
    expected = {"status": "ok", "core": {"db": "reachable"}, "plugins": []}
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/health").respond(json=jsend(expected))
        result = await health.check()
        assert result == expected


@pytest.mark.asyncio
async def test_health_check_server_error(health):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/health").respond(status_code=503)
        with pytest.raises(ServerError):
            await health.check()
