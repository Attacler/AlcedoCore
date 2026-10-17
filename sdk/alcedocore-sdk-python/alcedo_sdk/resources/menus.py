from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class MenusResource(BaseResource):
    """Menus resource. Maps to /api/app/menus (app-scoped)."""

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/app/menus", app=app, version=version)

    async def get(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", f"/api/app/menus/{_enc(id)}", app=app, version=version)

    async def create(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("POST", "/api/app/menus", json=data, app=app, version=version)

    async def update(
        self,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT", f"/api/app/menus/{_enc(id)}", json=data, app=app, version=version
        )

    async def delete(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("DELETE", f"/api/app/menus/{_enc(id)}", app=app, version=version)

    async def my(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/app/menus/my", app=app, version=version)

    async def get_roles(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/app/menus/{_enc(id)}/roles", app=app, version=version
        )

    async def set_roles(
        self,
        id: str,
        role_ids: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT",
            f"/api/app/menus/{_enc(id)}/roles",
            json={"role_ids": role_ids},
            app=app,
            version=version,
        )

    async def copy(
        self,
        id: str,
        source_menu_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            f"/api/app/menus/{_enc(id)}/copy",
            json={"source_menu_id": source_menu_id},
            app=app,
            version=version,
        )
