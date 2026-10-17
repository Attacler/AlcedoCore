import os
import warnings
from typing import Any, Optional

import httpx

from .resources.activity_logs import ActivityLogsResource
from .resources.app_settings import AppSettingsResource
from .resources.apps import AppsResource
from .resources.auth import AuthResource
from .resources.collections import CollectionsResource
from .resources.db import DBResource
from .resources.developer_keys import DeveloperApiKeysResource
from .resources.files import FilesResource
from .resources.health import HealthResource
from .resources.items import ItemsResource
from .resources.kv import KVResource
from .resources.lookup import LookupResource
from .resources.logs import LogsResource
from .resources.menus import MenusResource
from .resources.migrations import MigrationsResource
from .resources.plugins import PluginsResource
from .resources.policies import PoliciesResource
from .resources.registries import RegistriesResource
from .resources.roles import RolesResource
from .resources.schema import SchemaResource
from .resources.sessions import SessionsResource
from .resources.usage import UsageResource
from .resources.users import UsersResource
from .resources.versions import VersionsResource


class AlcedoClient:
    """Unified async-first client for Alcedo Plugin Core.

    Mirrors the node SDK's ``createClient``:

    - ``app``/``version`` become the default ``X-App``/``X-Version`` headers for
      app-scoped routes (``/api/app/*``); every resource method also accepts
      per-call ``app``/``version`` overrides.
    - ``request_id`` is sent as ``X-Request-ID`` (plugin callback identity).
    - ``api_key`` is sent as ``Authorization: Bearer <key>``.
    - Responses use the JSend envelope; resource methods return the unwrapped
      ``data`` payload and JSend ``fail``/``error`` raise
      :class:`alcedo_sdk.exceptions.AlcedoApiError`.

    Usage:
        async with AlcedoClient(plugin_slug="my-plugin", app="blog", version="production") as client:
            value = await client.kv.get("my-key")
            result = await client.db.query("SELECT * FROM items")
            settings = await client.app_settings.list()
    """

    def __init__(
        self,
        base_url: Optional[str] = None,
        plugin_slug: str = "system",
        timeout: float = 30.0,
        max_keepalive: int = 5,
        max_connections: int = 10,
        request_id: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
        api_key: Optional[str] = None,
    ):
        self._base_url = (base_url or os.environ.get("CORE_URL", "http://localhost:8080")).rstrip("/")
        self._plugin_slug = plugin_slug
        self._request_id = request_id
        self._app = app
        self._version = version
        self._api_key = api_key
        self._timeout = httpx.Timeout(timeout)
        self._limits = httpx.Limits(
            max_keepalive_connections=max_keepalive,
            max_connections=max_connections,
        )
        self._client: Optional[httpx.AsyncClient] = None
        self._resources_initialized = False

        # Resource attributes — set in _init_resources()
        self.activity_logs: ActivityLogsResource
        self.app_settings: AppSettingsResource
        self.apps: AppsResource
        self.auth: AuthResource
        self.collections: CollectionsResource
        self.db: DBResource
        self.developer_api_keys: DeveloperApiKeysResource
        self.files: FilesResource
        self.health: HealthResource
        self.items: ItemsResource
        self.kv: KVResource
        self.lookup: LookupResource
        self.logs: LogsResource
        self.menus: MenusResource
        self.migrations: MigrationsResource
        self.plugins: PluginsResource
        self.policies: PoliciesResource
        self.registries: RegistriesResource
        self.roles: RolesResource
        self.schema: SchemaResource
        self.sessions: SessionsResource
        self.usage: UsageResource
        self.users: UsersResource
        self.versions: VersionsResource

    def _build_client(self) -> httpx.AsyncClient:
        """Build the shared httpx client with connection pooling and auto-header injection."""
        async def inject_headers(request: httpx.Request) -> None:
            if self._request_id:
                request.headers.setdefault("X-Request-ID", self._request_id)
            if self._api_key:
                request.headers.setdefault("Authorization", f"Bearer {self._api_key}")
            if self._app:
                request.headers.setdefault("X-App", self._app)
            if self._version:
                request.headers.setdefault("X-Version", self._version)

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
        app = self._app
        version = self._version
        self.activity_logs = ActivityLogsResource(client, slug, app, version)
        self.app_settings = AppSettingsResource(client, slug, app, version)
        self.apps = AppsResource(client, slug, app, version)
        self.auth = AuthResource(client, slug, app, version)
        self.collections = CollectionsResource(client, slug, app, version)
        self.db = DBResource(client, slug, app, version)
        self.developer_api_keys = DeveloperApiKeysResource(client, slug, app, version)
        self.files = FilesResource(client, slug, app, version, base_url=self._base_url)
        self.health = HealthResource(client, slug, app, version)
        self.items = ItemsResource(client, slug, app, version)
        self.kv = KVResource(client, slug, app, version)
        self.lookup = LookupResource(client, slug, app, version)
        self.logs = LogsResource(client, slug, app, version)
        self.menus = MenusResource(client, slug, app, version)
        self.migrations = MigrationsResource(client, slug, app, version)
        self.plugins = PluginsResource(client, slug, app, version)
        self.policies = PoliciesResource(client, slug, app, version)
        self.registries = RegistriesResource(client, slug, app, version)
        self.roles = RolesResource(client, slug, app, version)
        self.schema = SchemaResource(client, slug, app, version)
        self.sessions = SessionsResource(client, slug, app, version)
        self.usage = UsageResource(client, slug, app, version)
        self.users = UsersResource(client, slug, app, version)
        self.versions = VersionsResource(client, slug, app, version)
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
    """DEPRECATED: Use AlcedoClient instead. This wrapper delegates to AlcedoClient.kv.

    Method return values preserve the old wrapper's semantics (plain values /
    booleans), extracted from the new JSend data payloads.
    """

    def __init__(
        self,
        base_url: str = "http://localhost:8080",
        plugin_slug: str = "system",
        timeout: float = 5.0,
        request_id: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
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
            app=app,
            version=version,
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

    @staticmethod
    def _field(data: Any, key: str, default: Any = None) -> Any:
        return data.get(key, default) if isinstance(data, dict) else default

    async def get(self, key: str, request_id: Optional[str] = None) -> Optional[Any]:
        self._check_client()
        await self._set_request_id(request_id)
        data = await self._client.kv.get(key)
        return self._field(data, "value")

    async def set(self, key: str, value: Any, ttl: Optional[int] = None, request_id: Optional[str] = None) -> Any:
        self._check_client()
        await self._set_request_id(request_id)
        data = await self._client.kv.set(key, value, ttl=ttl)
        return self._field(data, "value")

    async def delete(self, key: str, request_id: Optional[str] = None) -> bool:
        self._check_client()
        await self._set_request_id(request_id)
        data = await self._client.kv.delete(key)
        return bool(self._field(data, "deleted", True))

    async def exists(self, key: str, request_id: Optional[str] = None) -> bool:
        self._check_client()
        await self._set_request_id(request_id)
        data = await self._client.kv.exists(key)
        return bool(self._field(data, "exists"))

    async def ttl(self, key: str, request_id: Optional[str] = None) -> Optional[int]:
        self._check_client()
        await self._set_request_id(request_id)
        data = await self._client.kv.ttl(key)
        return self._field(data, "ttl")

    async def list_keys(self, prefix: str = "", request_id: Optional[str] = None) -> list:
        self._check_client()
        await self._set_request_id(request_id)
        data = await self._client.kv.list(prefix=prefix or None)
        return self._field(data, "keys", [])

    async def batch_get(self, keys: list, request_id: Optional[str] = None) -> dict:
        self._check_client()
        await self._set_request_id(request_id)
        data = await self._client.kv.batch_get(keys)
        return self._field(data, "values", {})

    async def batch_set(self, pairs: list, request_id: Optional[str] = None) -> None:
        self._check_client()
        await self._set_request_id(request_id)
        await self._client.kv.batch_set(pairs)

    async def batch_delete(self, keys: list, request_id: Optional[str] = None) -> int:
        self._check_client()
        await self._set_request_id(request_id)
        data = await self._client.kv.batch_delete(keys)
        return self._field(data, "deleted", 0)

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
