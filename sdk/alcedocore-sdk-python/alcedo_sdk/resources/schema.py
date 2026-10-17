from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class SchemaResource(BaseResource):
    """Plugin schema introspection resource.

    Maps to GET /api/platform/plugins/{slug}/schema.
    """

    async def get(
        self,
        slug: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Get the database schema for a plugin.

        Args:
            slug: Plugin slug; defaults to the client's plugin_slug.
        Returns dict with keys: plugin_name, schema_name, tables (list of table definitions).
        """
        resolved_slug = slug if slug is not None else self._plugin_slug
        return await self._request(
            "GET",
            f"/api/platform/plugins/{_enc(resolved_slug)}/schema",
            app=app,
            version=version,
        )
