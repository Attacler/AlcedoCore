import warnings

import pytest
import respx

from alcedo_sdk import AlcedoKV
from alcedo_sdk.client import AlcedoClient
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


class TestAlcedoKVDeprecation:

    def test_deprecation_warning_on_init(self):
        """AlcedoKV constructor raises DeprecationWarning."""
        with warnings.catch_warnings(record=True) as w:
            warnings.simplefilter("always")
            kv = AlcedoKV(plugin_slug="test")
            assert len(w) >= 1
            assert any(
                issubclass(x.category, DeprecationWarning)
                for x in w
            )
            assert any(
                "AlcedoKV is deprecated" in str(x.message)
                for x in w
            )

    def test_creates_alcedo_client(self):
        """AlcedoKV internally creates an AlcedoClient instance."""
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            kv = AlcedoKV(plugin_slug="test")
            assert isinstance(kv._client, AlcedoClient)

    @pytest.mark.asyncio
    async def test_context_manager_lifecycle(self):
        """AlcedoKV context manager delegates to AlcedoClient."""
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            async with AlcedoKV(plugin_slug="test") as kv:
                assert kv._client._client is not None

    def test_has_deprecated_methods(self):
        """AlcedoKV exposes all expected methods."""
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            kv = AlcedoKV(plugin_slug="test")
            for method in ["get", "set", "delete", "exists", "ttl",
                           "list_keys", "batch_get", "batch_set", "batch_delete",
                           "query"]:
                assert hasattr(kv, method), f"Missing method: {method}"


class TestAlcedoKVDelegation:
    """The deprecated wrapper extracts the old-style values from JSend payloads."""

    @pytest.fixture
    def kv(self):
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            return AlcedoKV(base_url=BASE_URL, plugin_slug="test")

    @pytest.mark.asyncio
    async def test_get_returns_value(self, kv):
        async with kv:
            with respx.mock as mock:
                mock.get(f"{BASE_URL}/api/app/kv/my-key").respond(
                    json=jsend({"key": "my-key", "value": "hello"})
                )
                assert await kv.get("my-key") == "hello"

    @pytest.mark.asyncio
    async def test_exists_returns_bool(self, kv):
        async with kv:
            with respx.mock as mock:
                mock.get(f"{BASE_URL}/api/app/kv/my-key/exists").respond(
                    json=jsend({"key": "my-key", "exists": True})
                )
                assert await kv.exists("my-key") is True

    @pytest.mark.asyncio
    async def test_list_keys_returns_list(self, kv):
        async with kv:
            with respx.mock as mock:
                mock.get(f"{BASE_URL}/api/app/kv").respond(
                    json=jsend({"keys": ["a", "b"]})
                )
                assert await kv.list_keys() == ["a", "b"]

    @pytest.mark.asyncio
    async def test_query_hits_proxy_endpoint(self, kv):
        async with kv:
            with respx.mock as mock:
                route = mock.post(f"{BASE_URL}/p/test/db/query").respond(
                    json=jsend({"columns": ["id"], "rows": [], "row_count": 0})
                )
                result = await kv.query("SELECT 1")
                assert result["row_count"] == 0
                assert route.called
