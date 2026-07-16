import httpx
import pytest
import respx

from alcedo_sdk.resources.settings import SettingsResource
from alcedo_sdk.exceptions import NotFoundError

BASE_URL = "http://localhost:8080"


@pytest.fixture
def settings():
    return SettingsResource(httpx.AsyncClient(base_url=BASE_URL), "test-plugin")


@pytest.mark.asyncio
async def test_get_settings(settings):
    expected = {"settings": {"greeting": "Hello"}, "schema": {}}
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/plugins/test-plugin/settings").respond(json=expected, status_code=200)
        result = await settings.get()
        assert result == expected


@pytest.mark.asyncio
async def test_get_settings_not_found(settings):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/plugins/test-plugin/settings").respond(status_code=404)
        with pytest.raises(NotFoundError):
            await settings.get()


@pytest.mark.asyncio
async def test_update_settings(settings):
    new_settings = {"greeting": "Shalom"}
    with respx.mock as mock:
        mock.patch(
            f"{BASE_URL}/api/plugins/test-plugin/settings",
            json=new_settings,
        ).respond(json={"settings": new_settings, "schema": {}}, status_code=200)
        result = await settings.update(new_settings)
        assert result["settings"] == {"greeting": "Shalom"}
