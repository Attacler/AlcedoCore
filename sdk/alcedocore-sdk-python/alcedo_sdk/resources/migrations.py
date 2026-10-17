from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class MigrationsResource(BaseResource):
    """Plugin database migrations resource.

    Maps to /api/platform/plugins/{slug}/migrations and
    /api/platform/plugins/{slug}/rollback/{version}.
    """

    async def list(
        self,
        slug: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """List all migrations for a plugin and their status.

        Args:
            slug: Plugin slug; defaults to the client's plugin_slug.
        """
        resolved_slug = slug if slug is not None else self._plugin_slug
        return await self._request(
            "GET",
            f"/api/platform/plugins/{_enc(resolved_slug)}/migrations",
            app=app,
            version=version,
        )

    async def run(
        self,
        slug: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> None:
        """Run pending migrations. Returns None (mirrors the node SDK)."""
        resolved_slug = slug if slug is not None else self._plugin_slug
        await self._request(
            "POST",
            f"/api/platform/plugins/{_enc(resolved_slug)}/migrations",
            app=app,
            version=version,
        )

    async def rollback(
        self,
        version: str,
        slug: Optional[str] = None,
        app: Optional[str] = None,
    ) -> Any:
        """Rollback a specific migration version.

        Args:
            version: Migration version string (e.g., "002_add_status_column")
            slug: Plugin slug; defaults to the client's plugin_slug.
        Returns: dict with rollback result.

        A per-call X-Version header can be supplied via the request ``headers``
        (the client-level default always applies when unset).
        """
        resolved_slug = slug if slug is not None else self._plugin_slug
        return await self._request(
            "POST",
            f"/api/platform/plugins/{_enc(resolved_slug)}/rollback/{_enc(version)}",
            app=app,
        )
