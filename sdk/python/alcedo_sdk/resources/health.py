from .base import BaseResource


class HealthResource(BaseResource):
    """Plugin core health check resource. Maps to GET /health."""

    async def check(self) -> dict:
        """Get the health status of plugin-core.

        Returns: { "status": str, "core": { "db": str, "docker": str, ... }, "plugins": [...] }
        """
        # HealthResource doesn't use plugin_slug — health is global
        return await self._request("GET", "/health")
