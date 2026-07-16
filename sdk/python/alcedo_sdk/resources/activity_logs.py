from typing import Any, Optional

from .base import BaseResource


class ActivityLogsResource(BaseResource):
    async def list_collections(self, params: Optional[dict[str, str]] = None) -> dict:
        return await self._request("GET", "/api/activity-logs/collections", params=params)

    async def list_system(self, params: Optional[dict[str, str]] = None) -> dict:
        return await self._request("GET", "/api/activity-logs/system", params=params)
