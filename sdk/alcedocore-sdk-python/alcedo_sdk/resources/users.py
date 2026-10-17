from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class UsersResource(BaseResource):
    """Users resource. Platform CRUD under /api/platform/users, app-scoped
    role assignment under /api/app/users/{id}/roles."""

    async def list(
        self,
        params: Optional[dict[str, Any]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request("GET", "/api/platform/users", params=params, app=app, version=version)

    async def get(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request("GET", f"/api/platform/users/{_enc(id)}", app=app, version=version)

    async def create(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request("POST", "/api/platform/users", json=data, app=app, version=version)

    async def update(
        self,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request(
            "PUT", f"/api/platform/users/{_enc(id)}", json=data, app=app, version=version
        )

    async def delete(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request("DELETE", f"/api/platform/users/{_enc(id)}", app=app, version=version)

    async def change_password(
        self,
        id: str,
        current_password: Optional[str] = None,
        new_password: Optional[str] = None,
        data: Optional[dict] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        body = data if data is not None else {
            "new_password": new_password,
        }
        if data is None and current_password is not None:
            body["current_password"] = current_password
        return await self._request(
            "POST", f"/api/platform/users/{_enc(id)}/password", json=body, app=app, version=version
        )

    async def sessions(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/platform/users/{_enc(id)}/sessions", app=app, version=version
        )

    async def revoke_session(
        self,
        id: str,
        session_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE",
            f"/api/platform/users/{_enc(id)}/sessions/{_enc(session_id)}",
            app=app,
            version=version,
        )

    async def revoke_all_sessions(
        self,
        id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE", f"/api/platform/users/{_enc(id)}/sessions", app=app, version=version
        )

    # --- app-scoped role assignment (X-App/X-Version required) ---

    async def list_roles(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", f"/api/app/users/{_enc(id)}/roles", app=app, version=version)

    async def assign_role(
        self,
        id: str,
        role_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            f"/api/app/users/{_enc(id)}/roles",
            json={"role_id": role_id},
            app=app,
            version=version,
        )

    async def remove_role(
        self,
        id: str,
        role_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE",
            f"/api/app/users/{_enc(id)}/roles/{_enc(role_id)}",
            app=app,
            version=version,
        )
