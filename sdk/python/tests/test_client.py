import httpx
import pytest

from alcedo_sdk import AlcedoClient


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
        """AlcedoClient exposes all 8 resource modules."""
        async with AlcedoClient(plugin_slug="test") as client:
            assert hasattr(client, "kv")
            assert hasattr(client, "db")
            assert hasattr(client, "settings")
            assert hasattr(client, "migrations")
            assert hasattr(client, "schema")
            assert hasattr(client, "logs")
            assert hasattr(client, "dev")
            assert hasattr(client, "health")


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
            assert client.settings._client is client._client
