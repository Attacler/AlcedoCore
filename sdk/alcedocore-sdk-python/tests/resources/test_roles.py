import pytest
import respx

from alcedo_sdk.resources.roles import RolesResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
ROLES = f"{BASE_URL}/api/app/roles"


@pytest.fixture
def roles(http_client):
    return RolesResource(http_client)


@pytest.mark.asyncio
async def test_crud(roles):
    with respx.mock as mock:
        listing = mock.get(f"{ROLES}").respond(json=jsend([{"id": "r1", "name": "customer"}]))
        fetched = mock.get(f"{ROLES}/r1").respond(json=jsend({"id": "r1"}))
        created = mock.post(f"{ROLES}", json={"name": "customer"}).respond(json=jsend({"id": "r1"}))
        updated = mock.put(f"{ROLES}/r1", json={"name": "client"}).respond(json=jsend({"id": "r1"}))
        deleted = mock.delete(f"{ROLES}/r1").respond(json=jsend({"success": True}))
        await roles.list()
        await roles.get("r1")
        await roles.create({"name": "customer"})
        await roles.update("r1", {"name": "client"})
        await roles.delete("r1")
        assert listing.called and fetched.called and created.called and updated.called and deleted.called


@pytest.mark.asyncio
async def test_permissions(roles):
    with respx.mock as mock:
        listing = mock.get(f"{ROLES}/r1/permissions").respond(json=jsend([]))
        updated = mock.post(f"{ROLES}/r1/permissions", json={"permissions": []}).respond(
            json=jsend({"success": True})
        )
        deleted = mock.delete(f"{ROLES}/r1/permissions/p1").respond(json=jsend({"success": True}))
        await roles.list_permissions("r1")
        await roles.update_permissions("r1", {"permissions": []})
        await roles.delete_permission("r1", "p1")
        assert listing.called and updated.called and deleted.called


@pytest.mark.asyncio
async def test_policies(roles):
    with respx.mock as mock:
        listing = mock.get(f"{ROLES}/r1/policies").respond(json=jsend([]))
        assigned = mock.post(f"{ROLES}/r1/policies", json={"policy_id": "pol1"}).respond(
            json=jsend({"success": True})
        )
        removed = mock.delete(f"{ROLES}/r1/policies/pol1").respond(json=jsend({"success": True}))
        await roles.list_policies("r1")
        await roles.assign_policy("r1", "pol1")
        await roles.remove_policy("r1", "pol1")
        assert listing.called and assigned.called and removed.called
