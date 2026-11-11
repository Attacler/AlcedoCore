from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class RouteMappingsResource(BaseResource):
    """Route mappings resource. Maps to /api/platform/settings/route-mappings.

    Mirrors the node SDK's ``routeMappings`` resource.
    """

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", "/api/platform/settings/route-mappings", app=app, version=version
        )

    async def create(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "POST", "/api/platform/settings/route-mappings", json=data, app=app, version=version
        )

    async def update(
        self,
        id: int,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT",
            f"/api/platform/settings/route-mappings/{_enc(id)}",
            json=data,
            app=app,
            version=version,
        )

    async def delete(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "DELETE", f"/api/platform/settings/route-mappings/{_enc(id)}", app=app, version=version
        )
