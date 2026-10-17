import pytest
import respx

from alcedo_sdk.resources.apps import AppsResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
APPS = f"{BASE_URL}/api/platform/apps"


@pytest.fixture
def apps(http_client):
    return AppsResource(http_client)


@pytest.mark.asyncio
async def test_crud(apps):
    with respx.mock as mock:
        listing = mock.get(f"{APPS}").respond(json=jsend([{"id": 1, "name": "Blog"}]))
        fetched = mock.get(f"{APPS}/1").respond(json=jsend({"id": 1, "name": "Blog"}))
        created = mock.post(f"{APPS}", json={"name": "Blog", "api_name": "blog", "version": "production"}).respond(
            json=jsend({"id": 1})
        )
        updated = mock.put(f"{APPS}/1", json={"name": "Blog2"}).respond(json=jsend({"id": 1}))
        removed = mock.delete(f"{APPS}/1").respond(json=jsend({"success": True}))
        await apps.list()
        await apps.get(1)
        await apps.create({"name": "Blog", "api_name": "blog", "version": "production"})
        await apps.update(1, {"name": "Blog2"})
        await apps.remove(1)
        assert listing.called and fetched.called and created.called and updated.called and removed.called


@pytest.mark.asyncio
async def test_me_apps(apps):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/platform/me/apps").respond(json=jsend([{"app_id": 1, "roles": []}]))
        result = await apps.me()
        assert result == [{"app_id": 1, "roles": []}]


@pytest.mark.asyncio
async def test_app_versions(apps):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/platform/app-versions").respond(
            json=jsend([{"id": 1, "app_id": 1, "version_name": "production"}])
        )
        result = await apps.app_versions()
        assert result == [{"id": 1, "app_id": 1, "version_name": "production"}]


@pytest.mark.asyncio
async def test_user_access(apps):
    with respx.mock as mock:
        get_route = mock.get(f"{BASE_URL}/api/platform/users/u1/app-access").respond(
            json=jsend([{"app_id": 1}])
        )
        put_route = mock.put(
            f"{BASE_URL}/api/platform/users/u1/app-access", json={"app_id": 1, "roles": ["admin"]}
        ).respond(json=jsend({"success": True}))
        await apps.get_user_access("u1")
        await apps.set_user_access("u1", {"app_id": 1, "roles": ["admin"]})
        assert get_route.called and put_route.called
