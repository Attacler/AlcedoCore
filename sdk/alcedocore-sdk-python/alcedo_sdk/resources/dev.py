from typing import Any, Optional

from .base import BaseResource


class DevResource(BaseResource):
    """Dev proxy helpers. Maps to /api/dev/* (not app- or platform-scoped).

    Mirrors the node SDK's ``dev`` resource. ``POST /api/dev/request-id``
    returns a bare ``{"request_id": ...}`` body (not the JSend envelope);
    ``_handle_response`` passes non-envelope bodies through as-is.
    """

    async def request_id(
        self,
        slug: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        """Mint a request id for the given plugin slug. Returns {"request_id": ...}."""
        return await self._request(
            "POST", "/api/dev/request-id", json={"slug": slug}, app=app, version=version
        )
