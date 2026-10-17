from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class RolesResource(BaseResource):
    """Roles resource. Maps to /api/app/roles (app-scoped)."""

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/app/roles", app=app, version=version)

    async def get(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", f"/api/app/roles/{_enc(id)}", app=app, version=version)

    async def create(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("POST", "/api/app/roles", json=data, app=app, version=version)

    async def update(
        self,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request("PUT", f"/api/app/roles/{_enc(id)}", json=data, app=app, version=version)

    async def delete(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("DELETE", f"/api/app/roles/{_enc(id)}", app=app, version=version)

    async def list_permissions(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/app/roles/{_enc(id)}/permissions", app=app, version=version
        )

    async def update_permissions(
        self,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST", f"/api/app/roles/{_enc(id)}/permissions", json=data, app=app, version=version
        )

    async def delete_permission(
        self,
        id: str,
        permission_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE",
            f"/api/app/roles/{_enc(id)}/permissions/{_enc(permission_id)}",
            app=app,
            version=version,
        )

    async def list_policies(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/app/roles/{_enc(id)}/policies", app=app, version=version
        )

    async def assign_policy(
        self,
        id: str,
        policy_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            f"/api/app/roles/{_enc(id)}/policies",
            json={"policy_id": policy_id},
            app=app,
            version=version,
        )

    async def remove_policy(
        self,
        id: str,
        policy_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE",
            f"/api/app/roles/{_enc(id)}/policies/{_enc(policy_id)}",
            app=app,
            version=version,
        )
