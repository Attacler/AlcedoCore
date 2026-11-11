import pytest
import respx

from alcedo_sdk.resources.dev import DevResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def dev(http_client):
    return DevResource(http_client, "test-plugin")


@pytest.mark.asyncio
async def test_request_id_returns_bare_body(dev):
    """POST /api/dev/request-id returns bare {request_id}, not a JSend envelope."""
    with respx.mock as mock:
        route = mock.post(
            f"{BASE_URL}/api/dev/request-id", json={"slug": "hello-world"}
        ).respond(json={"request_id": "req-123"})
        result = await dev.request_id("hello-world")
        assert result == {"request_id": "req-123"}
        assert route.called


@pytest.mark.asyncio
async def test_request_id_sends_app_headers(dev):
    with respx.mock as mock:
        route = mock.post(f"{BASE_URL}/api/dev/request-id", json={"slug": "hello-world"}).respond(
            json={"request_id": "req-9"}
        )
        await dev.request_id("hello-world", app="blog", version="production")
        request = route.calls.last.request
        assert request.headers["X-App"] == "blog"
        assert request.headers["X-Version"] == "production"


@pytest.mark.asyncio
async def test_request_id_jsend_error_still_raises(dev):
    from alcedo_sdk.exceptions import AlcedoApiError

    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/dev/request-id").respond(
            json={"status": "error", "message": "admin required"}, status_code=403
        )
        with pytest.raises(AlcedoApiError, match="admin required"):
            await dev.request_id("hello-world")
