from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class VersionsResource(BaseResource):
    """Versions resource. Maps to /api/platform/versions."""

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/platform/versions", app=app, version=version)

    async def create(
        self,
        version_name: str,
        data: Optional[dict] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        body = data if data is not None else {"version_name": version_name}
        return await self._request(
            "POST", "/api/platform/versions", json=body, app=app, version=version
        )

    async def remove(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request(
            "DELETE", f"/api/platform/versions/{_enc(id)}", app=app, version=version
        )
