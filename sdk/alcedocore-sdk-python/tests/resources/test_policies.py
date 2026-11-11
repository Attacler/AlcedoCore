import pytest
import respx

from alcedo_sdk.resources.policies import PoliciesResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
POLICIES = f"{BASE_URL}/api/app/policies"


@pytest.fixture
def policies(http_client):
    return PoliciesResource(http_client)


@pytest.mark.asyncio
async def test_crud(policies):
    with respx.mock as mock:
        listing = mock.get(f"{POLICIES}").respond(json=jsend([{"id": "pol1", "name": "cust"}]))
        fetched = mock.get(f"{POLICIES}/pol1").respond(json=jsend({"id": "pol1"}))
        created = mock.post(f"{POLICIES}", json={"name": "cust"}).respond(json=jsend({"id": "pol1"}))
        updated = mock.put(f"{POLICIES}/pol1", json={"name": "cust2"}).respond(json=jsend({"id": "pol1"}))
        deleted = mock.delete(f"{POLICIES}/pol1").respond(json=jsend({"success": True}))
        await policies.list()
        await policies.get("pol1")
        await policies.create({"name": "cust"})
        await policies.update("pol1", {"name": "cust2"})
        await policies.delete("pol1")
        assert listing.called and fetched.called and created.called and updated.called and deleted.called


@pytest.mark.asyncio
async def test_permissions(policies):
    with respx.mock as mock:
        listing = mock.get(f"{POLICIES}/pol1/permissions").respond(json=jsend([]))
        created = mock.post(f"{POLICIES}/pol1/permissions", json={"collection": "orders"}).respond(
            json=jsend({"id": "perm1"})
        )
        updated = mock.put(f"{POLICIES}/pol1/permissions/perm1", json={"actions": ["read"]}).respond(
            json=jsend({"id": "perm1"})
        )
        deleted = mock.delete(f"{POLICIES}/pol1/permissions/perm1").respond(json=jsend({"success": True}))
        collection_deleted = mock.delete(f"{POLICIES}/pol1/permissions/collection/orders").respond(
            json=jsend({"success": True})
        )
        await policies.list_permissions("pol1")
        await policies.create_permission("pol1", {"collection": "orders"})
        await policies.update_permission("pol1", "perm1", {"actions": ["read"]})
        await policies.delete_permission("pol1", "perm1")
        await policies.delete_collection_permissions("pol1", "orders")
        assert listing.called and created.called and updated.called and deleted.called
        assert collection_deleted.called


@pytest.mark.asyncio
async def test_plugin_policies(policies):
    with respx.mock as mock:
        assigned_plugins = mock.get(f"{POLICIES}/pol1/plugins").respond(json=jsend([]))
        plugin_policies = mock.get(f"{BASE_URL}/api/app/plugins/hello-world/policies").respond(
            json=jsend([])
        )
        assigned = mock.post(
            f"{BASE_URL}/api/app/plugins/hello-world/policies", json={"policy_id": "pol1"}
        ).respond(json=jsend({"success": True}))
        unassigned = mock.delete(f"{BASE_URL}/api/app/plugins/hello-world/policies/pol1").respond(
            json=jsend({"success": True})
        )
        await policies.list_assigned_plugins("pol1")
        await policies.list_plugin_policies("hello-world")
        await policies.assign_plugin_policy("hello-world", "pol1")
        await policies.unassign_plugin_policy("hello-world", "pol1")
        assert assigned_plugins.called and plugin_policies.called
        assert assigned.called and unassigned.called
