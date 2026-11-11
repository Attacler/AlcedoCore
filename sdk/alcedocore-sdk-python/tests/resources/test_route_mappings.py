import pytest
import respx

from alcedo_sdk.resources.route_mappings import RouteMappingsResource
from alcedo_sdk.exceptions import NotFoundError
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
ROUTE_MAPPINGS = f"{BASE_URL}/api/platform/settings/route-mappings"


@pytest.fixture
def route_mappings(http_client):
    return RouteMappingsResource(http_client, "test-plugin")


@pytest.mark.asyncio
async def test_list(route_mappings):
    with respx.mock as mock:
        mock.get(ROUTE_MAPPINGS).respond(json=jsend([{"id": 1, "install_id": 7}]))
        result = await route_mappings.list()
        assert result == [{"id": 1, "install_id": 7}]


@pytest.mark.asyncio
async def test_create(route_mappings):
    with respx.mock as mock:
        route = mock.post(
            ROUTE_MAPPINGS,
            json={"domain": "example.com", "path_prefix": "/blog", "install_id": 7},
        ).respond(json=jsend({"id": 1, "install_id": 7}))
        result = await route_mappings.create(
            {"domain": "example.com", "path_prefix": "/blog", "install_id": 7}
        )
        assert result == {"id": 1, "install_id": 7}
        assert route.called


@pytest.mark.asyncio
async def test_update(route_mappings):
    with respx.mock as mock:
        route = mock.put(f"{ROUTE_MAPPINGS}/1", json={"is_active": False}).respond(
            json=jsend({"id": 1, "is_active": False})
        )
        result = await route_mappings.update(1, {"is_active": False})
        assert result == {"id": 1, "is_active": False}
        assert route.called


@pytest.mark.asyncio
async def test_delete(route_mappings):
    with respx.mock as mock:
        route = mock.delete(f"{ROUTE_MAPPINGS}/1").respond(json=jsend({"deleted": True}))
        result = await route_mappings.delete(1)
        assert result == {"deleted": True}
        assert route.called


@pytest.mark.asyncio
async def test_not_found(route_mappings):
    with respx.mock as mock:
        mock.delete(f"{ROUTE_MAPPINGS}/99").respond(status_code=404)
        with pytest.raises(NotFoundError):
            await route_mappings.delete(99)
