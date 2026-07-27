from .base import BaseResource


class SchemaResource(BaseResource):
    """Plugin schema introspection resource. Maps to GET /api/plugins/{slug}/schema."""

    async def get(self) -> dict:
        """Get the database schema for this plugin.

        Returns dict with keys: plugin_name, schema_name, tables (list of table definitions).
        """
        return await self._request("GET", f"/api/plugins/{self._plugin_slug}/schema")
