from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class SessionsResource(BaseResource):
    """Own sessions resource. Maps to /api/platform/sessions."""

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/platform/sessions", app=app, version=version)

    async def revoke(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "DELETE", f"/api/platform/sessions/{_enc(id)}", app=app, version=version
        )

    async def revoke_all(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("DELETE", "/api/platform/sessions", app=app, version=version)
