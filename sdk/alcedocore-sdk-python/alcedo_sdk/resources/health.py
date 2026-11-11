from typing import Any, Optional

from .base import BaseResource


class HealthResource(BaseResource):
    """Core health check resource. Maps to GET /api/health."""

    async def check(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        """Get the health status of the core."""
        return await self._request("GET", "/api/health", app=app, version=version)
