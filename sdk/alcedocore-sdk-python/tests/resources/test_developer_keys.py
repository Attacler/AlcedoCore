import pytest
import respx

from alcedo_sdk.resources.developer_keys import DeveloperApiKeysResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
KEYS = f"{BASE_URL}/api/platform/developer-keys"


@pytest.fixture
def keys(http_client):
    return DeveloperApiKeysResource(http_client)


@pytest.mark.asyncio
async def test_list(keys):
    with respx.mock as mock:
        mock.get(f"{KEYS}").respond(json=jsend([{"id": "k1", "name": "ci"}]))
        result = await keys.list()
        assert result == [{"id": "k1", "name": "ci"}]


@pytest.mark.asyncio
async def test_list_scoped_to_version(keys):
    with respx.mock as mock:
        route = mock.get(f"{KEYS}", params={"version_id": "3"}).respond(json=jsend([]))
        await keys.list(version_id=3)
        assert route.called


@pytest.mark.asyncio
async def test_create(keys):
    with respx.mock as mock:
        mock.post(f"{KEYS}", json={"name": "ci", "version_id": 3}).respond(
            json=jsend({"id": "k1", "raw_key": "alc_..."})
        )
        result = await keys.create(3, "ci")
        assert result["raw_key"] == "alc_..."


@pytest.mark.asyncio
async def test_remove(keys):
    with respx.mock as mock:
        mock.delete(f"{KEYS}/k1").respond(json=jsend({"success": True}))
        result = await keys.remove("k1")
        assert result == {"success": True}
