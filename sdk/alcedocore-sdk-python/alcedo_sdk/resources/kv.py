import json
from typing import Any, Optional

from .base import BaseResource


class KVResource(BaseResource):
    """Key-Value store resource. Maps to /api/app/kv/* endpoints.

    App-scoped: requires X-App/X-Version headers (set via client defaults or
    per-call ``app``/``version`` arguments).
    """

    async def get(self, key: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        """Get a value by key. Returns the API payload ({"key": ..., "value": ...})."""
        return await self._request("GET", f"/api/app/kv/{key}", app=app, version=version)

    async def set(
        self,
        key: str,
        value: Any,
        ttl: Optional[int] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Set a key-value pair with optional TTL (seconds).

        Objects are JSON-encoded before sending (mirrors the node SDK).
        Returns the API payload ({"key": ..., "value": ...}).
        """
        payload: dict[str, Any] = {
            "value": value if isinstance(value, (str, bytes)) else json.dumps(value)
        }
        params = {"ttl": str(ttl)} if ttl is not None else None
        return await self._request(
            "PUT", f"/api/app/kv/{key}", json=payload, params=params, app=app, version=version
        )

    async def delete(self, key: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        """Delete a key. Returns the API payload ({"key": ..., "deleted": true})."""
        return await self._request("DELETE", f"/api/app/kv/{key}", app=app, version=version)

    async def exists(self, key: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        """Check if a key exists. Returns the API payload ({"key": ..., "exists": bool})."""
        return await self._request("GET", f"/api/app/kv/{key}/exists", app=app, version=version)

    async def ttl(self, key: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        """Get TTL for a key in seconds. Returns the API payload ({"key": ..., "ttl": int|None})."""
        return await self._request("GET", f"/api/app/kv/{key}/ttl", app=app, version=version)

    async def list(
        self,
        prefix: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """List keys, optionally filtered by prefix. Returns {"keys": [...]}."""
        params = {"prefix": prefix} if prefix is not None else None
        return await self._request("GET", "/api/app/kv", params=params, app=app, version=version)

    # Backward-compatible alias for the old Python SDK name.
    list_keys = list

    async def batch_get(
        self,
        keys: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Get multiple keys at once. Returns {"values": {...}}."""
        return await self._request(
            "POST", "/api/app/kv/batch/get", json={"keys": keys}, app=app, version=version
        )

    async def batch_set(
        self,
        pairs: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Set multiple key-value pairs at once.

        Each pair: {"key": str, "value": any, "ttl": optional int}.
        """
        return await self._request(
            "POST", "/api/app/kv/batch/set", json=pairs, app=app, version=version
        )

    async def batch_delete(
        self,
        keys: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Delete multiple keys at once. Returns {"deleted": count}."""
        return await self._request(
            "POST", "/api/app/kv/batch/delete", json={"keys": keys}, app=app, version=version
        )

    async def query(
        self,
        pattern: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Query keys by pattern. Returns {"status": "ok"} or matched keys."""
        params = {"pattern": pattern} if pattern is not None else None
        return await self._request(
            "GET", "/api/app/kv/query", params=params, app=app, version=version
        )
