import pytest
import respx

from alcedo_sdk.resources.registries import RegistriesResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
REGISTRIES = f"{BASE_URL}/api/platform/registries"


@pytest.fixture
def registries(http_client):
    return RegistriesResource(http_client)


@pytest.mark.asyncio
async def test_crud(registries):
    with respx.mock as mock:
        listing = mock.get(f"{REGISTRIES}").respond(json=jsend([{"id": 1, "name": "local"}]))
        fetched = mock.get(f"{REGISTRIES}/1").respond(json=jsend({"id": 1, "name": "local"}))
        created = mock.post(f"{REGISTRIES}", json={"name": "local", "url": "http://localhost:5000"}).respond(
            json=jsend({"id": 2})
        )
        updated = mock.put(f"{REGISTRIES}/1", json={"name": "local2"}).respond(json=jsend({"id": 1}))
        removed = mock.delete(f"{REGISTRIES}/1").respond(json=jsend({"success": True}))
        await registries.list()
        await registries.get(1)
        await registries.create({"name": "local", "url": "http://localhost:5000"})
        await registries.update(1, {"name": "local2"})
        await registries.delete(1)
        assert listing.called and fetched.called and created.called and updated.called and removed.called


@pytest.mark.asyncio
async def test_health_check(registries):
    with respx.mock as mock:
        mock.get(f"{REGISTRIES}/1/health").respond(json=jsend({"healthy": True}))
        result = await registries.health_check(1)
        assert result == {"healthy": True}


@pytest.mark.asyncio
async def test_health_check_url(registries):
    with respx.mock as mock:
        mock.post(f"{REGISTRIES}/health-check", json={"url": "http://localhost:5000"}).respond(
            json=jsend({"healthy": True})
        )
        result = await registries.health_check_url("http://localhost:5000")
        assert result == {"healthy": True}


@pytest.mark.asyncio
async def test_images(registries):
    with respx.mock as mock:
        mock.get(f"{REGISTRIES}/1/images").respond(json=jsend([{"name": "hello-world"}]))
        result = await registries.images(1)
        assert result == [{"name": "hello-world"}]
