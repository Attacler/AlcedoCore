import pytest
import respx

from alcedo_sdk.resources.schema import SchemaResource
from alcedo_sdk.exceptions import ServerError
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def schema(http_client):
    return SchemaResource(http_client, "test-plugin")


@pytest.mark.asyncio
async def test_get_schema(schema):
    expected = {
        "plugin_name": "test-plugin",
        "schema_name": "plugin_test_plugin",
        "tables": [{"table_name": "items", "columns": [{"name": "id", "type": "integer"}]}],
    }
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/platform/plugins/test-plugin/schema").respond(
            json=jsend(expected)
        )
        result = await schema.get()
        assert result == expected


@pytest.mark.asyncio
async def test_get_schema_explicit_slug(schema):
    with respx.mock as mock:
        route = mock.get(f"{BASE_URL}/api/platform/plugins/other/schema").respond(json=jsend({}))
        await schema.get(slug="other")
        assert route.called


@pytest.mark.asyncio
async def test_get_schema_server_error(schema):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/platform/plugins/test-plugin/schema").respond(status_code=500)
        with pytest.raises(ServerError):
            await schema.get()
