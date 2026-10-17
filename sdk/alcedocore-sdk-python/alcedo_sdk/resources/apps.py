from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class AppsResource(BaseResource):
    """Apps resource. Maps to /api/platform/apps and app-version deploy targets."""

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/platform/apps", app=app, version=version)

    async def get(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request("GET", f"/api/platform/apps/{_enc(id)}", app=app, version=version)

    async def create(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request("POST", "/api/platform/apps", json=data, app=app, version=version)

    async def update(
        self,
        id: int,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request(
            "PUT", f"/api/platform/apps/{_enc(id)}", json=data, app=app, version=version
        )

    async def remove(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request("DELETE", f"/api/platform/apps/{_enc(id)}", app=app, version=version)

    async def me(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        """Apps the current user has access to."""
        return await self._request("GET", "/api/platform/me/apps", app=app, version=version)

    async def app_versions(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        """Every app x version pair (deploy targets for plugins)."""
        return await self._request("GET", "/api/platform/app-versions", app=app, version=version)

    async def get_user_access(
        self,
        user_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", f"/api/platform/users/{_enc(user_id)}/app-access", app=app, version=version
        )

    async def set_user_access(
        self,
        user_id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT",
            f"/api/platform/users/{_enc(user_id)}/app-access",
            json=data,
            app=app,
            version=version,
        )
