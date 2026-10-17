import httpx
import pytest
import respx

from alcedo_sdk import AlcedoClient
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"

ALL_RESOURCES = [
    "plugins",
    "health",
    "activity_logs",
    "app_settings",
    "developer_api_keys",
    "menus",
    "registries",
    "migrations",
    "usage",
    "kv",
    "db",
    "schema",
    "logs",
    "auth",
    "sessions",
    "apps",
    "versions",
    "users",
    "roles",
    "policies",
    "collections",
    "items",
    "lookup",
    "files",
]


class TestAlcedoClientLifecycle:

    @pytest.mark.asyncio
    async def test_async_context_manager(self):
        """AlcedoClient can be used as async context manager."""
        async with AlcedoClient(plugin_slug="test") as client:
            assert client._client is not None
            assert not client._client.is_closed

    @pytest.mark.asyncio
    async def test_manual_aclose(self):
        """AlcedoClient can be manually opened and closed."""
        client = AlcedoClient(plugin_slug="test")
        await client.__aenter__()
        assert client._client is not None
        await client.aclose()
        assert client._client is None

    @pytest.mark.asyncio
    async def test_aclose_idempotent(self):
        """Calling aclose() multiple times is safe."""
        client = AlcedoClient(plugin_slug="test")
        await client.__aenter__()
        await client.aclose()
        await client.aclose()  # second call should not raise

    @pytest.mark.asyncio
    async def test_base_url_from_env(self, monkeypatch):
        """CORE_URL env var is used as default base_url."""
        monkeypatch.setenv("CORE_URL", "http://test-core:9090")
        async with AlcedoClient(plugin_slug="test") as client:
            assert client._base_url == "http://test-core:9090"

    @pytest.mark.asyncio
    async def test_base_url_default(self):
        """Default base_url is http://localhost:8080."""
        async with AlcedoClient(plugin_slug="test") as client:
            assert client._base_url == "http://localhost:8080"


class TestAlcedoClientHeaderInjection:

    @pytest.mark.asyncio
    async def test_event_hooks_configured(self):
        """Client has event_hooks with request hook."""
        async with AlcedoClient(plugin_slug="test") as client:
            hooks = client._client._event_hooks
            assert "request" in hooks
            assert len(hooks["request"]) == 1

    @pytest.mark.asyncio
    async def test_resource_modules_available(self):
        """AlcedoClient exposes every resource from the node SDK surface."""
        async with AlcedoClient(plugin_slug="test") as client:
            for name in ALL_RESOURCES:
                assert hasattr(client, name), f"Missing resource: {name}"

    @pytest.mark.asyncio
    async def test_app_version_headers_sent(self):
        """Default app/version become X-App/X-Version headers."""
        async with AlcedoClient(
            plugin_slug="test", app="blog", version="production"
        ) as client:
            with respx.mock as mock:
                route = mock.get(f"{BASE_URL}/api/app/kv/my-key").respond(json=jsend({"key": "my-key", "value": "v"}))
                await client.kv.get("my-key")
                request = route.calls.last.request
                assert request.headers["X-App"] == "blog"
                assert request.headers["X-Version"] == "production"

    @pytest.mark.asyncio
    async def test_per_call_app_version_override(self):
        """Per-call app/version override the client defaults."""
        async with AlcedoClient(
            plugin_slug="test", app="blog", version="production"
        ) as client:
            with respx.mock as mock:
                route = mock.get(f"{BASE_URL}/api/app/kv/my-key").respond(json=jsend({"key": "my-key", "value": "v"}))
                await client.kv.get("my-key", app="shop", version="staging")
                request = route.calls.last.request
                assert request.headers["X-App"] == "shop"
                assert request.headers["X-Version"] == "staging"

    @pytest.mark.asyncio
    async def test_request_id_and_api_key_headers(self):
        """request_id → X-Request-ID; api_key → Authorization Bearer."""
        async with AlcedoClient(
            plugin_slug="test", request_id="req-123", api_key="secret-key"
        ) as client:
            with respx.mock as mock:
                route = mock.get(f"{BASE_URL}/api/health").respond(json=jsend({"status": "ok"}))
                await client.health.check()
                request = route.calls.last.request
                assert request.headers["X-Request-ID"] == "req-123"
                assert request.headers["Authorization"] == "Bearer secret-key"

    @pytest.mark.asyncio
    async def test_no_request_id_header_when_unset(self):
        """X-Request-ID is only sent when configured (mirrors the node client)."""
        async with AlcedoClient(plugin_slug="test") as client:
            with respx.mock as mock:
                route = mock.get(f"{BASE_URL}/api/health").respond(json=jsend({"status": "ok"}))
                await client.health.check()
                request = route.calls.last.request
                assert "X-Request-ID" not in request.headers


class TestAlcedoClientConnectionPooling:

    @pytest.mark.asyncio
    async def test_default_limits(self):
        """Default connection pooling limits are configured."""
        async with AlcedoClient(plugin_slug="test") as client:
            limits = client._limits
            assert limits.max_keepalive_connections == 5
            assert limits.max_connections == 10

    @pytest.mark.asyncio
    async def test_custom_limits(self):
        """Custom connection pooling limits are accepted."""
        async with AlcedoClient(plugin_slug="test", max_keepalive=10, max_connections=20) as client:
            limits = client._limits
            assert limits.max_keepalive_connections == 10
            assert limits.max_connections == 20

    @pytest.mark.asyncio
    async def test_shared_client(self):
        """All resource modules share the same httpx client."""
        async with AlcedoClient(plugin_slug="test") as client:
            assert client.kv._client is client._client
            assert client.db._client is client._client
            assert client.plugins._client is client._client


class TestJSendUnwrapping:

    @pytest.mark.asyncio
    async def test_success_envelope_unwrapped(self):
        """JSend success responses return the data payload."""
        async with AlcedoClient(plugin_slug="test") as client:
            with respx.mock as mock:
                mock.get(f"{BASE_URL}/api/health").respond(json=jsend({"status": "ok"}))
                result = await client.health.check()
                assert result == {"status": "ok"}

    @pytest.mark.asyncio
    async def test_error_envelope_raises_alcedo_api_error(self):
        """JSend fail/error envelopes raise AlcedoApiError."""
        from alcedo_sdk import AlcedoApiError

        async with AlcedoClient(plugin_slug="test") as client:
            with respx.mock as mock:
                mock.get(f"{BASE_URL}/api/health").respond(
                    json={"status": "error", "message": "core down", "code": 500}, status_code=500
                )
                with pytest.raises(AlcedoApiError) as exc_info:
                    await client.health.check()
                assert exc_info.value.message == "core down"
                assert exc_info.value.code == 500
                assert exc_info.value.status == "error"
                assert exc_info.value.http_status == 500
