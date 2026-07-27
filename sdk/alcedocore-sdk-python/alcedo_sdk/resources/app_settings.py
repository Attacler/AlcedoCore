from typing import Any, Optional

from .base import BaseResource


class AppSettingsResource(BaseResource):
    async def list(self) -> dict:
        return await self._request("GET", "/api/app-settings")

    async def update(self, key: str, value: Any) -> dict:
        return await self._request("PUT", f"/api/app-settings/{key}", json={"value": value})

    async def batch(self, settings: dict) -> dict:
        return await self._request("PUT", "/api/app-settings", json={"settings": settings})
