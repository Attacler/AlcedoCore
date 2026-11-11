import pytest
import respx

from alcedo_sdk.resources.lookup import LookupResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def lookup(http_client):
    return LookupResource(http_client)


@pytest.mark.asyncio
async def test_users(lookup):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/lookup/users").respond(
            json=jsend({"data": [{"id": "u1", "email": "a@b.c"}]})
        )
        result = await lookup.users()
        assert result == {"data": [{"id": "u1", "email": "a@b.c"}]}


@pytest.mark.asyncio
async def test_users_app_headers(lookup):
    with respx.mock as mock:
        route = mock.get(f"{BASE_URL}/api/app/lookup/users").respond(json=jsend({"data": []}))
        await lookup.users(app="blog", version="production")
        request = route.calls.last.request
        assert request.headers["X-App"] == "blog"
        assert request.headers["X-Version"] == "production"
