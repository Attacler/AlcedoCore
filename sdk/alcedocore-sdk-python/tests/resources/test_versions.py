import pytest
import respx

from alcedo_sdk.resources.versions import VersionsResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
VERSIONS = f"{BASE_URL}/api/platform/versions"


@pytest.fixture
def versions(http_client):
    return VersionsResource(http_client)


@pytest.mark.asyncio
async def test_list(versions):
    with respx.mock as mock:
        mock.get(f"{VERSIONS}").respond(json=jsend([{"id": 1, "version_name": "production"}]))
        result = await versions.list()
        assert result == [{"id": 1, "version_name": "production"}]


@pytest.mark.asyncio
async def test_create(versions):
    with respx.mock as mock:
        mock.post(f"{VERSIONS}", json={"version_name": "staging"}).respond(
            json=jsend({"id": 2, "version_name": "staging"})
        )
        result = await versions.create("staging")
        assert result == {"id": 2, "version_name": "staging"}


@pytest.mark.asyncio
async def test_remove(versions):
    with respx.mock as mock:
        mock.delete(f"{VERSIONS}/2").respond(json=jsend({"success": True}))
        result = await versions.remove(2)
        assert result == {"success": True}
