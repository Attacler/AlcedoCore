import httpx
import pytest
import respx

from alcedo_sdk.resources.dev import DevResource
from alcedo_sdk.exceptions import ServerError

BASE_URL = "http://localhost:8080"


@pytest.fixture
def dev():
    return DevResource(httpx.AsyncClient(base_url=BASE_URL), "test-plugin")


@pytest.mark.asyncio
async def test_start_dev(dev):
    expected = {"slug": "test-plugin", "url": "http://localhost:3000", "ttl_secs": 3600}
    with respx.mock as mock:
        mock.post(
            f"{BASE_URL}/api/dev/start",
            json={"slug": "test-plugin", "url": "http://localhost:3000", "ttl_secs": 3600},
        ).respond(json=expected, status_code=200)
        result = await dev.start(url="http://localhost:3000")
        assert result == expected


@pytest.mark.asyncio
async def test_start_dev_custom_ttl(dev):
    with respx.mock as mock:
        mock.post(
            f"{BASE_URL}/api/dev/start",
            json={"slug": "test-plugin", "url": "http://localhost:5173", "ttl_secs": 7200},
        ).respond(json={}, status_code=200)
        result = await dev.start(url="http://localhost:5173", ttl_secs=7200)
        assert result == {}


@pytest.mark.asyncio
async def test_stop_dev(dev):
    with respx.mock as mock:
        mock.post(
            f"{BASE_URL}/api/dev/stop",
            json={"slug": "test-plugin"},
        ).respond(json={"status": "stopped"}, status_code=200)
        result = await dev.stop()
        assert result == {"status": "stopped"}


@pytest.mark.asyncio
async def test_stop_dev_error(dev):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/dev/stop", json={"slug": "test-plugin"}).respond(status_code=500)
        with pytest.raises(ServerError):
            await dev.stop()
