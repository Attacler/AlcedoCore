import pytest
import respx

from alcedo_sdk.resources.auth import AuthResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def auth(http_client):
    return AuthResource(http_client)


@pytest.mark.asyncio
async def test_me(auth):
    user = {"id": "u1", "email": "admin@alcedo.dev", "display_name": "Admin", "is_admin": True}
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/platform/auth/me").respond(json=jsend({"user": user, "scopes": []}))
        result = await auth.me()
        assert result == {"user": user, "scopes": []}


@pytest.mark.asyncio
async def test_login(auth):
    with respx.mock as mock:
        mock.post(
            f"{BASE_URL}/api/platform/auth/login",
            json={"email": "admin@alcedo.dev", "password": "admin123!"},
        ).respond(json=jsend({"user": {"id": "u1", "email": "admin@alcedo.dev"}}))
        result = await auth.login("admin@alcedo.dev", "admin123!")
        assert result["user"]["email"] == "admin@alcedo.dev"


@pytest.mark.asyncio
async def test_logout(auth):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/platform/auth/logout").respond(json=jsend({"success": True}))
        result = await auth.logout()
        assert result == {"success": True}
