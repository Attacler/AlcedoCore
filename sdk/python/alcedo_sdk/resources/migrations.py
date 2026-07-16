from typing import Any

from .base import BaseResource


class MigrationsResource(BaseResource):
    """Plugin database migrations resource. Maps to /api/plugins/{slug}/migrations and rollback."""

    async def list(self) -> list[dict]:
        """List all migrations for this plugin and their status.

        Returns: [{ "version": str, "name": str, "status": str, "applied_at": str, ... }]
        """
        data = await self._request("GET", f"/api/plugins/{self._plugin_slug}/migrations")
        return data if isinstance(data, list) else data.get("migrations", [])

    async def run(self) -> dict:
        """Run pending migrations.

        Returns: { "applied": [str], "errors": [str] }
        """
        return await self._request("POST", f"/api/plugins/{self._plugin_slug}/migrations")

    async def rollback(self, version: str) -> dict:
        """Rollback a specific migration version.

        Args:
            version: Migration version string (e.g., "002_add_status_column")
        Returns: dict with rollback result.
        """
        return await self._request("POST", f"/api/plugins/{self._plugin_slug}/rollback/{version}")
