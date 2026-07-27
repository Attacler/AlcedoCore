from typing import Any, Optional
from urllib.parse import urlparse

from ..exceptions import ValidationError
from .base import BaseResource


class DevResource(BaseResource):
    """Plugin dev session resource. Maps to /api/dev/start and /api/dev/stop."""

    async def start(self, url: str, ttl_secs: int = 3600) -> dict:
        """Start a dev session for this plugin.

        Args:
            url: Local development URL (e.g., "http://localhost:3000")
            ttl_secs: Session TTL in seconds (default 1 hour)
        Returns: Dev session info dict.

        Raises:
            ValidationError: If the URL scheme is not http or https.
        """
        parsed = urlparse(url)
        if parsed.scheme not in ("http", "https"):
            raise ValidationError(
                f"Invalid URL scheme '{parsed.scheme}': only http/https URLs are allowed",
            )
        return await self._request(
            "POST",
            "/api/dev/start",
            json={"slug": self._plugin_slug, "url": url, "ttl_secs": ttl_secs},
        )

    async def stop(self) -> dict:
        """Stop the active dev session for this plugin."""
        return await self._request(
            "POST",
            "/api/dev/stop",
            json={"slug": self._plugin_slug},
        )
