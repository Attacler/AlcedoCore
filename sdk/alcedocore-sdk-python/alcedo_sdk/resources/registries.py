from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class RegistriesResource(BaseResource):
    """Docker registries resource. Maps to /api/platform/registries."""

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/platform/registries", app=app, version=version)

    async def get(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/platform/registries/{_enc(id)}", app=app, version=version
        )

    async def create(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "POST", "/api/platform/registries", json=data, app=app, version=version
        )

    async def update(
        self,
        id: int,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT", f"/api/platform/registries/{_enc(id)}", json=data, app=app, version=version
        )

    async def delete(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "DELETE", f"/api/platform/registries/{_enc(id)}", app=app, version=version
        )

    async def health_check(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/platform/registries/{_enc(id)}/health", app=app, version=version
        )

    async def health_check_url(
        self,
        url: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            "/api/platform/registries/health-check",
            json={"url": url},
            app=app,
            version=version,
        )

    async def images(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/platform/registries/{_enc(id)}/images", app=app, version=version
        )
