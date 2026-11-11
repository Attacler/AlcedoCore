import pytest
import respx

from alcedo_sdk.resources.menus import MenusResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
MENUS = f"{BASE_URL}/api/app/menus"


@pytest.fixture
def menus(http_client):
    return MenusResource(http_client)


@pytest.mark.asyncio
async def test_crud(menus):
    with respx.mock as mock:
        listing = mock.get(f"{MENUS}").respond(
            json=jsend([{"id": "m1", "name": "main", "role_count": 1, "item_count": 2}])
        )
        fetched = mock.get(f"{MENUS}/m1").respond(json=jsend({"id": "m1", "sections": []}))
        created = mock.post(f"{MENUS}", json={"name": "main"}).respond(json=jsend({"id": "m1"}))
        updated = mock.put(f"{MENUS}/m1", json={"name": "main2"}).respond(json=jsend({"id": "m1"}))
        deleted = mock.delete(f"{MENUS}/m1").respond(json=jsend({"success": True}))
        await menus.list()
        await menus.get("m1")
        await menus.create({"name": "main"})
        await menus.update("m1", {"name": "main2"})
        await menus.delete("m1")
        assert listing.called and fetched.called and created.called and updated.called and deleted.called


@pytest.mark.asyncio
async def test_my_menus(menus):
    with respx.mock as mock:
        mock.get(f"{MENUS}/my").respond(json=jsend([{"id": "m1", "sections": []}]))
        result = await menus.my()
        assert result == [{"id": "m1", "sections": []}]


@pytest.mark.asyncio
async def test_roles(menus):
    with respx.mock as mock:
        get_route = mock.get(f"{MENUS}/m1/roles").respond(json=jsend(["r1"]))
        set_route = mock.put(f"{MENUS}/m1/roles", json={"role_ids": ["r1", "r2"]}).respond(
            json=jsend({"success": True})
        )
        await menus.get_roles("m1")
        await menus.set_roles("m1", ["r1", "r2"])
        assert get_route.called and set_route.called


@pytest.mark.asyncio
async def test_copy(menus):
    with respx.mock as mock:
        mock.post(f"{MENUS}/m1/copy", json={"source_menu_id": "m0"}).respond(json=jsend({"id": "m1"}))
        result = await menus.copy("m1", "m0")
        assert result == {"id": "m1"}
