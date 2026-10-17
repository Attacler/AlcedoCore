import pytest
import respx

from alcedo_sdk.resources.activity_logs import ActivityLogsResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def activity_logs(http_client):
    return ActivityLogsResource(http_client)


@pytest.mark.asyncio
async def test_list_collections(activity_logs):
    expected = {"data": [{"id": "l1", "collection_name": "orders"}], "total": 1, "limit": 50, "offset": 0}
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/logs/collections").respond(json=jsend(expected))
        result = await activity_logs.list_collections()
        assert result == expected


@pytest.mark.asyncio
async def test_list_app_system(activity_logs):
    expected = {"data": [{"id": 1, "target": "core"}], "total": 1, "limit": 50, "offset": 0}
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/logs/system").respond(json=jsend(expected))
        result = await activity_logs.list_app_system()
        assert result == expected


@pytest.mark.asyncio
async def test_list_system_platform(activity_logs):
    expected = {"data": [{"id": 1, "target": "core"}], "total": 1, "limit": 50, "offset": 0}
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/platform/logs").respond(json=jsend(expected))
        result = await activity_logs.list_system()
        assert result == expected


@pytest.mark.asyncio
async def test_params_and_app_headers(activity_logs):
    with respx.mock as mock:
        route = mock.get(
            f"{BASE_URL}/api/app/logs/collections",
            params={"limit": "10", "offset": "5"},
        ).respond(json=jsend({"data": [], "total": 0, "limit": 10, "offset": 5}))
        await activity_logs.list_collections(
            params={"limit": "10", "offset": "5"}, app="blog", version="production"
        )
        request = route.calls.last.request
        assert request.headers["X-App"] == "blog"
        assert request.headers["X-Version"] == "production"
