import pytest
import respx

from alcedo_sdk.resources.logs import LogsResource
from alcedo_sdk.exceptions import ServerError
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def logs(http_client):
    return LogsResource(http_client, "test-plugin")


@pytest.mark.asyncio
async def test_list_logs(logs):
    expected = [{"id": 1, "method": "GET", "path": "/api/app/kv/test", "status": 200}]
    with respx.mock as mock:
        mock.get(
            f"{BASE_URL}/api/platform/plugins/test-plugin/logs",
        ).respond(json=jsend(expected))
        result = await logs.list(limit=50, offset=0)
        assert result == expected


@pytest.mark.asyncio
async def test_list_logs_with_filters(logs):
    with respx.mock as mock:
        mock.get(
            f"{BASE_URL}/api/platform/plugins/test-plugin/logs",
            params={
                "limit": "10",
                "offset": "5",
                "start_date": "2025-01-01",
                "operation_type": "create",
            },
        ).respond(json=jsend([]))
        result = await logs.list(limit=10, offset=5, start_date="2025-01-01", operation_type="create")
        assert result == []


@pytest.mark.asyncio
async def test_list_logs_explicit_slug(logs):
    with respx.mock as mock:
        route = mock.get(f"{BASE_URL}/api/platform/plugins/other/logs").respond(json=jsend([]))
        await logs.list(slug="other")
        assert route.called


@pytest.mark.asyncio
async def test_list_logs_server_error(logs):
    with respx.mock as mock:
        mock.get(
            f"{BASE_URL}/api/platform/plugins/test-plugin/logs",
            params={"limit": "50", "offset": "0"},
        ).respond(status_code=500)
        with pytest.raises(ServerError):
            await logs.list(limit=50, offset=0)
