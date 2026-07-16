import json
from typing import Any, Optional

from ..exceptions import NotFoundError, ValidationError
from .base import BaseResource


class KVResource(BaseResource):
    """Key-Value store resource. Maps to /api/kv/* endpoints."""

    async def get(self, key: str) -> Optional[Any]:
        """Get a value by key. Returns None if key doesn't exist."""
        data = await self._request("GET", f"/api/kv/{key}")
        return data.get("data") if data else None

    async def set(self, key: str, value: Any, ttl: Optional[int] = None) -> Any:
        """Set a key-value pair with optional TTL (seconds). Returns the stored value."""
        payload: dict[str, Any] = {"value": value if isinstance(value, (str, bytes)) else json.dumps(value)}
        params = {"ttl": str(ttl)} if ttl is not None else None
        data = await self._request("PUT", f"/api/kv/{key}", json=payload, params=params)
        return data.get("data") if data else None

    async def delete(self, key: str) -> bool:
        """Delete a key. Returns True if deleted, False if key didn't exist."""
        try:
            await self._request("DELETE", f"/api/kv/{key}")
            return True
        except NotFoundError:
            return False

    async def exists(self, key: str) -> bool:
        """Check if a key exists."""
        data = await self._request("GET", f"/api/kv/{key}/exists")
        return data.get("exists", False) if data else False

    async def ttl(self, key: str) -> Optional[int]:
        """Get TTL for a key in seconds. Returns None if no TTL set."""
        data = await self._request("GET", f"/api/kv/{key}/ttl")
        if data is None:
            return None
        val = data.get("ttl")
        return int(val) if val is not None else None

    async def list_keys(self, prefix: str = "") -> list[str]:
        """List all keys, optionally filtered by prefix."""
        params = {"prefix": prefix} if prefix else None
        data = await self._request("GET", "/api/kv/", params=params)
        return data.get("keys", []) if data else []

    async def batch_get(self, keys: list[str]) -> dict[str, Optional[Any]]:
        """Get multiple keys at once. Returns dict of key -> value (None for missing keys)."""
        data = await self._request("POST", "/api/kv/batch/get", json={"keys": keys})
        return data.get("values", {}) if data else {}

    async def batch_set(self, pairs: list[dict[str, Any]]) -> None:
        """Set multiple key-value pairs at once. Each pair: {"key": str, "value": any, "ttl": optional int}."""
        items: list[dict[str, Any]] = []
        for pair in pairs:
            if "key" not in pair:
                raise ValidationError(
                    "Each pair must contain a 'key' field",
                    details={"pair": pair},
                )
            if "value" not in pair:
                raise ValidationError(
                    "Each pair must contain a 'value' field",
                    details={"pair": pair},
                )
            item: dict[str, Any] = {
                "key": pair["key"],
                "value": str(pair["value"]),
            }
            if "ttl" in pair:
                item["ttl"] = pair["ttl"]
            items.append(item)
        await self._request("POST", "/api/kv/batch/set", json=items)

    async def batch_delete(self, keys: list[str]) -> int:
        """Delete multiple keys at once. Returns count of deleted keys."""
        data = await self._request("POST", "/api/kv/batch/delete", json={"keys": keys})
        return data.get("deleted", 0) if data else 0
