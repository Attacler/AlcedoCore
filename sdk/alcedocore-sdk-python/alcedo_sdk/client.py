import os
import uuid
import warnings
from typing import Any, Optional

import httpx

from .resources.kv import KVResource
from .resources.db import DBResource
from .resources.settings import SettingsResource
from .resources.migrations import MigrationsResource
from .resources.schema import SchemaResource
from .resources.logs import LogsResource
from .resources.files import FilesResource
from .resources.health import HealthResource
from .resources.items import ItemsResource


class AlcedoClient:
    """Unified async-first client for Alcedo Plugin Core.

    Usage:
        async with AlcedoClient(plugin_slug="my-plugin") as client:
            value = await client.kv.get("my-key")
            result = await client.db.query("SELECT * FROM items")
            settings = await client.settings.get()

    All resource modules (.kv, .db, .settings, .migrations, .schema,
    .logs, .health) share a single httpx.AsyncClient with
    connection pooling and auto-header injection.
    """

    def __init__(
        self,
        base_url: Optional[str] = None,
        plugin_slug: str = "system",
        timeout: float = 10.0,
        max_keepalive: int = 5,
        max_connections: int = 10,
        request_id: Optional[str] = None,
    ):
        self._base_url = (base_url or os.environ.get("CORE_URL", "http://localhost:8080")).rstrip("/")
        self._plugin_slug = plugin_slug
        self._request_id = request_id
        self._timeout = httpx.Timeout(timeout)
        self._limits = httpx.Limits(
            max_keepalive_connections=max_keepalive,
            max_connections=max_connections,
        )
        self._client: Optional[httpx.AsyncClient] = None
        self._resources_initialized = False

        # Resource attributes — set in _init_resources()
        self.kv: KVResource
        self.db: DBResource
        self.items: ItemsResource
        self.settings: SettingsResource
        self.migrations: MigrationsResource
        self.schema: SchemaResource
        self.logs: LogsResource
        self.files: FilesResource
        self.health: HealthResource

    def _build_client(self) -> httpx.AsyncClient:
        """Build the shared httpx client with connection pooling and auto-header injection."""
        async def inject_headers(request: httpx.Request) -> None:
            request.headers.setdefault("X-Plugin-Slug", self._plugin_slug)
            request.headers.setdefault("X-Request-ID", self._request_id or str(uuid.uuid4()))

        return httpx.AsyncClient(
            base_url=self._base_url,
            timeout=self._timeout,
            limits=self._limits,
            event_hooks={"request": [inject_headers]},
        )

    def _init_resources(self) -> None:
        """Initialize resource modules once the client is open."""
        if self._resources_initialized or self._client is None:
            return
        client = self._client
        slug = self._plugin_slug
        self.kv = KVResource(client, slug)
        self.db = DBResource(client, slug)
        self.items = ItemsResource(client, slug)
        self.settings = SettingsResource(client, slug)
        self.migrations = MigrationsResource(client, slug)
        self.schema = SchemaResource(client, slug)
        self.logs = LogsResource(client, slug)
        self.files = FilesResource(client, slug)
        self.health = HealthResource(client)
        self._resources_initialized = True

    async def __aenter__(self) -> "AlcedoClient":
        self._client = self._build_client()
        self._init_resources()
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.aclose()

    async def aclose(self) -> None:
        """Manually close the client (for non-context-manager usage).

        Safe to call multiple times — idempotent.
        """
        if self._client:
            await self._client.aclose()
            self._client = None
            self._resources_initialized = False


class AlcedoKV:
    """DEPRECATED: Use AlcedoClient instead. This wrapper delegates to AlcedoClient.kv."""

    def __init__(
        self,
        base_url: str = "http://localhost:8080",
        plugin_slug: str = "system",
        timeout: float = 5.0,
        request_id: Optional[str] = None,
    ):
        warnings.warn(
            "AlcedoKV is deprecated, use AlcedoClient instead",
            DeprecationWarning,
            stacklevel=2,
        )
        self._client = AlcedoClient(
            base_url=base_url,
            plugin_slug=plugin_slug,
            timeout=timeout,
            request_id=request_id,
        )

    async def __aenter__(self) -> "AlcedoKV":
        await self._client.__aenter__()
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self._client.__aexit__(*args)

    def _check_client(self) -> None:
        """Replicate existing behavior — raise RuntimeError if client not initialized."""
        if self._client._client is None:
            raise RuntimeError(
                "AlcedoKV client is not initialized. Use 'async with AlcedoKV(...) as kv:'"
            )

    # --- KV methods (delegated to self._client.kv) ---

    async def _set_request_id(self, request_id: Optional[str] = None) -> None:
        if request_id is not None:
            self._client._request_id = request_id

    async def get(self, key: str, request_id: Optional[str] = None) -> Optional[Any]:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.get(key)

    async def set(self, key: str, value: Any, ttl: Optional[int] = None, request_id: Optional[str] = None) -> Any:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.set(key, value, ttl=ttl)

    async def delete(self, key: str, request_id: Optional[str] = None) -> bool:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.delete(key)

    async def exists(self, key: str, request_id: Optional[str] = None) -> bool:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.exists(key)

    async def ttl(self, key: str, request_id: Optional[str] = None) -> Optional[int]:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.ttl(key)

    async def list_keys(self, prefix: str = "", request_id: Optional[str] = None) -> list[str]:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.list_keys(prefix=prefix)

    async def batch_get(self, keys: list[str], request_id: Optional[str] = None) -> dict[str, Optional[Any]]:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.batch_get(keys)

    async def batch_set(self, pairs: list[dict[str, Any]], request_id: Optional[str] = None) -> None:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.batch_set(pairs)

    async def batch_delete(self, keys: list[str], request_id: Optional[str] = None) -> int:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.kv.batch_delete(keys)

    async def query(self, sql: str, params: Optional[list] = None,
                    timeout_secs: int = 30, max_rows: int = 100,
                    request_id: Optional[str] = None) -> dict:
        self._check_client()
        await self._set_request_id(request_id)
        return await self._client.db.query(sql, params=params, timeout_secs=timeout_secs, max_rows=max_rows)

    async def compare_and_swap(self, key: str, old_value: str, new_value: str) -> bool:
        raise NotImplementedError("CAS is not yet implemented")

    async def increment(self, key: str, delta: int = 1) -> int:
        raise NotImplementedError("Increment is not yet implemented")
