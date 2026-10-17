import pytest
import respx

from alcedo_sdk.resources.users import UsersResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
USERS = f"{BASE_URL}/api/platform/users"


@pytest.fixture
def users(http_client):
    return UsersResource(http_client)


@pytest.mark.asyncio
async def test_list(users):
    with respx.mock as mock:
        mock.get(f"{USERS}").respond(json=jsend([{"id": "u1"}]))
        result = await users.list()
        assert result == [{"id": "u1"}]


@pytest.mark.asyncio
async def test_crud(users):
    with respx.mock as mock:
        created = mock.post(f"{USERS}", json={"email": "a@b.c"}).respond(json=jsend({"id": "u2"}))
        fetched = mock.get(f"{USERS}/u2").respond(json=jsend({"id": "u2"}))
        updated = mock.put(f"{USERS}/u2", json={"display_name": "A"}).respond(json=jsend({"id": "u2"}))
        deleted = mock.delete(f"{USERS}/u2").respond(json=jsend({"success": True}))
        await users.create({"email": "a@b.c"})
        await users.get("u2")
        await users.update("u2", {"display_name": "A"})
        await users.delete("u2")
        assert created.called and fetched.called and updated.called and deleted.called


@pytest.mark.asyncio
async def test_change_password(users):
    with respx.mock as mock:
        route = mock.post(
            f"{USERS}/u2/password",
            json={"current_password": "old", "new_password": "new"},
        ).respond(json=jsend({"success": True}))
        await users.change_password("u2", current_password="old", new_password="new")
        assert route.called


@pytest.mark.asyncio
async def test_sessions_management(users):
    with respx.mock as mock:
        listing = mock.get(f"{USERS}/u2/sessions").respond(json=jsend([{"id": "s1"}]))
        revoke = mock.delete(f"{USERS}/u2/sessions/s1").respond(json=jsend(True))
        revoke_all = mock.delete(f"{USERS}/u2/sessions").respond(json=jsend(True))
        await users.sessions("u2")
        await users.revoke_session("u2", "s1")
        await users.revoke_all_sessions("u2")
        assert listing.called and revoke.called and revoke_all.called


@pytest.mark.asyncio
async def test_list_roles_app_scoped(users):
    with respx.mock as mock:
        route = mock.get(f"{BASE_URL}/api/app/users/u2/roles").respond(json=jsend([{"id": "r1"}]))
        result = await users.list_roles("u2", app="blog", version="production")
        assert result == [{"id": "r1"}]
        request = route.calls.last.request
        assert request.headers["X-App"] == "blog"
        assert request.headers["X-Version"] == "production"


@pytest.mark.asyncio
async def test_assign_remove_role(users):
    with respx.mock as mock:
        assign = mock.post(
            f"{BASE_URL}/api/app/users/u2/roles", json={"role_id": "r1"}
        ).respond(json=jsend({"success": True}))
        remove = mock.delete(f"{BASE_URL}/api/app/users/u2/roles/r1").respond(json=jsend({"success": True}))
        await users.assign_role("u2", "r1")
        await users.remove_role("u2", "r1")
        assert assign.called and remove.called
