import httpx
import pytest
import respx

from alcedo_sdk.resources.schema import SchemaResource
from alcedo_sdk.exceptions import ServerError

BASE_URL = "http://localhost:8080"


@pytest.fixture
def schema():
    return SchemaResource(httpx.AsyncClient(base_url=BASE_URL), "test-plugin")


@pytest.mark.asyncio
async def test_get_schema(schema):
    expected = {
        "plugin_name": "test-plugin",
        "schema_name": "plugin_test_plugin",
        "tables": [{"table_name": "items", "columns": [{"name": "id", "type": "integer"}]}],
    }
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/plugins/test-plugin/schema").respond(json=expected, status_code=200)
        result = await schema.get()
        assert result == expected


@pytest.mark.asyncio
async def test_get_schema_server_error(schema):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/plugins/test-plugin/schema").respond(status_code=500)
        with pytest.raises(ServerError):
            await schema.get()
