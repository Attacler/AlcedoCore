import pytest
import respx

from alcedo_sdk.resources.app_settings import AppSettingsResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def app_settings(http_client):
    return AppSettingsResource(http_client)


@pytest.mark.asyncio
async def test_get_platform_settings(app_settings):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/platform/settings").respond(
            json=jsend({"platform_name": "Alcedo"})
        )
        result = await app_settings.get_platform_settings()
        assert result == {"platform_name": "Alcedo"}


@pytest.mark.asyncio
async def test_update_platform_settings(app_settings):
    with respx.mock as mock:
        mock.put(f"{BASE_URL}/api/platform/settings", json={"platform_name": "Acme"}).respond(
            json=jsend({"platform_name": "Acme"})
        )
        result = await app_settings.update_platform_settings("Acme")
        assert result == {"platform_name": "Acme"}


@pytest.mark.asyncio
async def test_list_app_settings(app_settings):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/settings").respond(json=jsend({"site_name": "Blog"}))
        result = await app_settings.list()
        assert result == {"site_name": "Blog"}


@pytest.mark.asyncio
async def test_update_app_setting(app_settings):
    with respx.mock as mock:
        mock.put(f"{BASE_URL}/api/app/settings/site_name", json={"value": "News"}).respond(
            json=jsend({"key": "site_name", "updated_at": "2026-01-01T00:00:00Z"})
        )
        result = await app_settings.update("site_name", "News")
        assert result["key"] == "site_name"


@pytest.mark.asyncio
async def test_batch_update(app_settings):
    with respx.mock as mock:
        mock.post(
            f"{BASE_URL}/api/app/settings/batch",
            json={"settings": {"site_name": "News", "logo_url": "https://x/y.png"}},
        ).respond(json=jsend({"count": 2, "success": True}))
        result = await app_settings.batch({"site_name": "News", "logo_url": "https://x/y.png"})
        assert result == {"count": 2, "success": True}


@pytest.mark.asyncio
async def test_app_scoped_headers(app_settings):
    with respx.mock as mock:
        route = mock.get(f"{BASE_URL}/api/app/settings").respond(json=jsend({}))
        await app_settings.list(app="blog", version="production")
        request = route.calls.last.request
        assert request.headers["X-App"] == "blog"
        assert request.headers["X-Version"] == "production"
