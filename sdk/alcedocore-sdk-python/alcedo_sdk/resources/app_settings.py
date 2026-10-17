from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class AppSettingsResource(BaseResource):
    """Platform + app settings. Maps to /api/platform/settings and /api/app/settings.

    Mirrors the node SDK's ``appSettings`` resource.
    """

    async def get_platform_settings(
        self,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request("GET", "/api/platform/settings", app=app, version=version)

    async def update_platform_settings(
        self,
        platform_name: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request(
            "PUT",
            "/api/platform/settings",
            json={"platform_name": platform_name},
            app=app,
            version=version,
        )

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request("GET", "/api/app/settings", app=app, version=version)

    async def update(
        self,
        key: str,
        value: Any,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request(
            "PUT",
            f"/api/app/settings/{_enc(key)}",
            json={"value": value},
            app=app,
            version=version,
        )

    async def batch(
        self,
        settings: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request(
            "POST",
            "/api/app/settings/batch",
            json={"settings": settings},
            app=app,
            version=version,
        )
