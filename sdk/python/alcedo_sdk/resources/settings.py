from typing import Any

from .base import BaseResource


class SettingsResource(BaseResource):
    """Plugin settings resource. Maps to /api/plugins/{slug}/settings."""

    async def get(self) -> dict:
        """Get all settings for this plugin.

        Returns: { "settings": {...}, "schema": {...} }
        """
        return await self._request("GET", f"/api/plugins/{self._plugin_slug}/settings")

    async def update(self, settings: dict[str, Any]) -> dict:
        """Update plugin settings. Sends the full settings object (replaces entirely).

        Args:
            settings: Dict of setting key -> value. Sends raw JSON body.
        Returns: Updated settings dict from API.
        """
        return await self._request("PATCH", f"/api/plugins/{self._plugin_slug}/settings", json=settings)
