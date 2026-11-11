from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


def _policy(id: str, suffix: str = "") -> str:
    return f"/api/app/policies/{_enc(id)}{suffix}"


class PoliciesResource(BaseResource):
    """Policies resource. Maps to /api/app/policies (app-scoped).

    Plugin-policy assignment lives under /api/app/plugins/{slug}/policies.
    """

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/app/policies", app=app, version=version)

    async def get(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _policy(id), app=app, version=version)

    async def create(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("POST", "/api/app/policies", json=data, app=app, version=version)

    async def update(
        self,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request("PUT", _policy(id), json=data, app=app, version=version)

    async def delete(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("DELETE", _policy(id), app=app, version=version)

    # --- permissions ---

    async def list_permissions(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _policy(id, "/permissions"), app=app, version=version)

    async def create_permission(
        self,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST", _policy(id, "/permissions"), json=data, app=app, version=version
        )

    async def update_permission(
        self,
        id: str,
        permission_id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT",
            _policy(id, f"/permissions/{_enc(permission_id)}"),
            json=data,
            app=app,
            version=version,
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
            _policy(id, f"/permissions/{_enc(permission_id)}"),
            app=app,
            version=version,
        )

    async def delete_collection_permissions(
        self,
        id: str,
        collection_name: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE",
            _policy(id, f"/permissions/collection/{_enc(collection_name)}"),
            app=app,
            version=version,
        )

    # --- plugin-policy assignment ---

    async def list_assigned_plugins(
        self,
        policy_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", _policy(policy_id, "/plugins"), app=app, version=version
        )

    async def list_plugin_policies(
        self,
        plugin_slug: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", f"/api/app/plugins/{_enc(plugin_slug)}/policies", app=app, version=version
        )

    async def assign_plugin_policy(
        self,
        plugin_slug: str,
        policy_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            f"/api/app/plugins/{_enc(plugin_slug)}/policies",
            json={"policy_id": policy_id},
            app=app,
            version=version,
        )

    async def unassign_plugin_policy(
        self,
        plugin_slug: str,
        policy_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE",
            f"/api/app/plugins/{_enc(plugin_slug)}/policies/{_enc(policy_id)}",
            app=app,
            version=version,
        )
