import json

import pytest
import respx

from alcedo_sdk.resources.items import ItemsResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
ITEMS = f"{BASE_URL}/api/app/items"


@pytest.fixture
def items(http_client):
    return ItemsResource(http_client, "test-plugin")


@pytest.mark.asyncio
async def test_list(items):
    expected = {"data": [{"id": "i1"}], "total": 1}
    with respx.mock as mock:
        mock.get(f"{ITEMS}/posts").respond(json=jsend(expected))
        result = await items.list("posts")
        assert result == expected


@pytest.mark.asyncio
async def test_list_users_goes_to_platform(items):
    """Listing the users collection routes through /api/platform/users."""
    with respx.mock as mock:
        route = mock.get(f"{BASE_URL}/api/platform/users").respond(json=jsend([{"id": "u1"}]))
        result = await items.list("users")
        assert result == [{"id": "u1"}]
        assert route.called


@pytest.mark.asyncio
async def test_crud(items):
    with respx.mock as mock:
        created = mock.post(f"{ITEMS}/posts", json={"title": "Hi"}).respond(json=jsend({"id": "i1"}))
        fetched = mock.get(f"{ITEMS}/posts/i1").respond(json=jsend({"id": "i1"}))
        patched = mock.patch(f"{ITEMS}/posts/i1", json={"title": "Yo"}).respond(json=jsend({"id": "i1"}))
        updated = mock.patch(
            f"{ITEMS}/posts",
            params={"filter": json.dumps({"id": "i1"})},
            json={"title": "Yo"},
        ).respond(json=jsend({"updated": 1}))
        deleted = mock.delete(f"{ITEMS}/posts", json={"pk_values": ["i1"]}).respond(
            json=jsend({"deleted": 1})
        )
        await items.create("posts", {"title": "Hi"})
        await items.get("posts", "i1")
        await items.patch("posts", "i1", {"title": "Yo"})
        await items.update("posts", {"filter": {"id": "i1"}, "update": {"title": "Yo"}})
        await items.delete("posts", {"pk_values": ["i1"]})
        assert created.called and fetched.called and patched.called and updated.called and deleted.called


@pytest.mark.asyncio
async def test_query_builds_params(items):
    with respx.mock as mock:
        route = mock.get(
            f"{ITEMS}/posts",
            params={
                "filter": '{"id": "i1"}',
                "limit": "10",
                "offset": "5",
                "fields": "id,title",
                "sort": "title",
                "order": "desc",
            },
        ).respond(json=jsend({"data": [], "total": 0}))
        await items.query(
            "posts",
            {
                "filter": {"id": "i1"},
                "limit": 10,
                "offset": 5,
                "fields": ["id", "title"],
                "sort": [{"field": "title", "order": "desc"}],
            },
        )
        assert route.called


@pytest.mark.asyncio
async def test_grouped_and_references(items):
    with respx.mock as mock:
        grouped = mock.post(f"{ITEMS}/posts/grouped", json={"group_by": "status"}).respond(
            json=jsend({"groups": []})
        )
        references = mock.get(f"{ITEMS}/posts/i1/references").respond(json=jsend([]))
        await items.grouped("posts", {"group_by": "status"})
        await items.references("posts", "i1")
        assert grouped.called and references.called


@pytest.mark.asyncio
async def test_permission_checks(items):
    with respx.mock as mock:
        delete_perm = mock.post(f"{ITEMS}/posts/$delete", json={"pk_values": ["i1"]}).respond(
            json=jsend({"i1": True})
        )
        update_perm = mock.post(f"{ITEMS}/posts/$update", json={"pk_values": ["i1"]}).respond(
            json=jsend({"i1": False})
        )
        item_perm = mock.post(
            f"{ITEMS}/posts/$permissions",
            json={"action": "update", "item": {"title": "x"}, "pk": "i1"},
        ).respond(json=jsend({"action": "update", "allowed": True, "rules": [], "violations": []}))
        await items.delete_permissions("posts", ["i1"])
        await items.update_permissions("posts", ["i1"])
        await items.permissions("posts", {"title": "x"}, action="update", pk="i1")
        assert delete_perm.called and update_perm.called and item_perm.called
