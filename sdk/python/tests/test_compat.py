import warnings

import pytest

from alcedo_sdk import AlcedoKV
from alcedo_sdk.client import AlcedoClient


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
