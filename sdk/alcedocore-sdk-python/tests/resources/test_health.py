import httpx
import pytest
import respx

from alcedo_sdk.resources.health import HealthResource
from alcedo_sdk.exceptions import ServerError

BASE_URL = "http://localhost:8080"


@pytest.fixture
def health():
    return HealthResource(httpx.AsyncClient(base_url=BASE_URL))


@pytest.mark.asyncio
async def test_health_check(health):
    expected = {"status": "ok", "core": {"db": "reachable"}, "plugins": []}
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/health").respond(json=expected, status_code=200)
        result = await health.check()
        assert result == expected


@pytest.mark.asyncio
async def test_health_check_server_error(health):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/health").respond(status_code=503)
        with pytest.raises(ServerError):
            await health.check()
