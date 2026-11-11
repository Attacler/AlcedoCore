from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class DeveloperApiKeysResource(BaseResource):
    """Developer API keys resource. Maps to /api/platform/developer-keys."""

    async def list(
        self,
        version_id: Optional[int] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """List developer keys, optionally scoped to a single version."""
        params = {"version_id": str(version_id)} if version_id is not None else None
        return await self._request(
            "GET", "/api/platform/developer-keys", params=params, app=app, version=version
        )

    async def create(
        self,
        version_id: int,
        name: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        """Create a key bound to a version. The raw key is only returned once."""
        return await self._request(
            "POST",
            "/api/platform/developer-keys",
            json={"name": name, "version_id": version_id},
            app=app,
            version=version,
        )

    async def remove(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request(
            "DELETE", f"/api/platform/developer-keys/{_enc(id)}", app=app, version=version
        )
