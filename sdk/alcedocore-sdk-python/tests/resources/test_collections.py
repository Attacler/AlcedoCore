import pytest
import respx

from alcedo_sdk.resources.collections import CollectionsResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
COLLECTIONS = f"{BASE_URL}/api/app/collections"


@pytest.fixture
def collections(http_client):
    return CollectionsResource(http_client, "test-plugin")


@pytest.mark.asyncio
async def test_crud(collections):
    with respx.mock as mock:
        listing = mock.get(f"{COLLECTIONS}").respond(json=jsend({"collections": []}))
        fetched = mock.get(f"{COLLECTIONS}/posts").respond(json=jsend({"name": "posts"}))
        created = mock.post(f"{COLLECTIONS}", json={"name": "posts", "fields": []}).respond(
            json=jsend({"name": "posts"})
        )
        updated = mock.put(f"{COLLECTIONS}/posts", json={"display_name": "Posts"}).respond(
            json=jsend({"success": True})
        )
        deleted = mock.delete(f"{COLLECTIONS}/posts").respond(json=jsend({"deleted": True}))
        await collections.list()
        await collections.get("posts")
        await collections.create({"name": "posts", "fields": []})
        await collections.update("posts", {"display_name": "Posts"})
        await collections.delete("posts")
        assert listing.called and fetched.called and created.called and updated.called and deleted.called


@pytest.mark.asyncio
async def test_layouts(collections):
    with respx.mock as mock:
        listing = mock.get(f"{COLLECTIONS}/posts/layouts").respond(json=jsend([]))
        created = mock.post(f"{COLLECTIONS}/posts/layouts", json={"name": "default"}).respond(
            json=jsend({"id": "lay1"})
        )
        updated = mock.put(f"{COLLECTIONS}/posts/layouts/lay1", json={"name": "main"}).respond(
            json=jsend({"id": "lay1"})
        )
        roles_get = mock.get(f"{COLLECTIONS}/posts/layouts/lay1/roles").respond(json=jsend([]))
        roles_put = mock.put(
            f"{COLLECTIONS}/posts/layouts/lay1/roles", json={"role_ids": ["r1"]}
        ).respond(json=jsend({"success": True}))
        resolved = mock.get(f"{COLLECTIONS}/posts/layout").respond(json=jsend({"id": "lay1"}))
        deleted = mock.delete(f"{COLLECTIONS}/posts/layouts/lay1").respond(json=jsend({"success": True}))
        await collections.list_layouts("posts")
        await collections.create_layout("posts", {"name": "default"})
        await collections.update_layout("posts", "lay1", {"name": "main"})
        await collections.get_layout_roles("posts", "lay1")
        await collections.set_layout_roles("posts", "lay1", ["r1"])
        await collections.get_resolved_layout("posts")
        await collections.delete_layout("posts", "lay1")
        assert listing.called and created.called and updated.called
        assert roles_get.called and roles_put.called and resolved.called and deleted.called


@pytest.mark.asyncio
async def test_layout_sections(collections):
    with respx.mock as mock:
        listing = mock.get(f"{COLLECTIONS}/posts/layouts/lay1/sections").respond(json=jsend([]))
        created = mock.post(
            f"{COLLECTIONS}/posts/layouts/lay1/sections", json={"label": "Main"}
        ).respond(json=jsend({"id": "sec1"}))
        updated = mock.put(
            f"{COLLECTIONS}/posts/layouts/lay1/sections/sec1", json={"label": "More"}
        ).respond(json=jsend({"id": "sec1"}))
        reordered = mock.patch(
            f"{COLLECTIONS}/posts/layouts/lay1/sections",
            json={"sections": [{"id": "sec1", "position": 0}]},
        ).respond(json=jsend({"success": True}))
        deleted = mock.delete(f"{COLLECTIONS}/posts/layouts/lay1/sections/sec1").respond(
            json=jsend({"success": True})
        )
        await collections.list_layout_sections("posts", "lay1")
        await collections.create_layout_section("posts", "lay1", {"label": "Main"})
        await collections.update_layout_section("posts", "lay1", "sec1", {"label": "More"})
        await collections.batch_reorder_sections("posts", "lay1", [{"id": "sec1", "position": 0}])
        await collections.delete_layout_section("posts", "lay1", "sec1")
        assert listing.called and created.called and updated.called and reordered.called and deleted.called


@pytest.mark.asyncio
async def test_views(collections):
    with respx.mock as mock:
        listing = mock.get(f"{COLLECTIONS}/posts/views").respond(json=jsend([]))
        created = mock.post(f"{COLLECTIONS}/posts/views", json={"name": "all"}).respond(
            json=jsend({"id": "v1"})
        )
        updated = mock.put(f"{COLLECTIONS}/posts/views/v1", json={"name": "all2"}).respond(
            json=jsend({"id": "v1"})
        )
        default = mock.put(f"{COLLECTIONS}/posts/views/v1/default").respond(json=jsend({"success": True}))
        deleted = mock.delete(f"{COLLECTIONS}/posts/views/v1").respond(json=jsend({"success": True}))
        await collections.list_views("posts")
        await collections.create_view("posts", {"name": "all"})
        await collections.update_view("posts", "v1", {"name": "all2"})
        await collections.set_default_view("posts", "v1")
        await collections.delete_view("posts", "v1")
        assert listing.called and created.called and updated.called and default.called and deleted.called


@pytest.mark.asyncio
async def test_get_create_policy(collections):
    with respx.mock as mock:
        mock.get(f"{COLLECTIONS}/posts/$create").respond(
            json=jsend({"$permissions": {"create": True}})
        )
        result = await collections.get_create_policy("posts")
        assert result == {"$permissions": {"create": True}}
