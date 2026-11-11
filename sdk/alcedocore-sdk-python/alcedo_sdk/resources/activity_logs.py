from typing import Any, Optional

from .base import BaseResource


class ActivityLogsResource(BaseResource):
    """Activity logs. App-scoped item/system activity under /api/app/logs,
    global platform activity under /api/platform/logs."""

    async def list_collections(
        self,
        params: Optional[dict[str, Any]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        """App-scoped item activity (GET /api/app/logs/collections)."""
        return await self._request(
            "GET", "/api/app/logs/collections", params=params, app=app, version=version
        )

    async def list_app_system(
        self,
        params: Optional[dict[str, Any]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        """App-scoped, non-item activity (GET /api/app/logs/system)."""
        return await self._request(
            "GET", "/api/app/logs/system", params=params, app=app, version=version
        )

    async def list_system(
        self,
        params: Optional[dict[str, Any]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        """Global/platform activity (GET /api/platform/logs)."""
        return await self._request(
            "GET", "/api/platform/logs", params=params, app=app, version=version
        )
