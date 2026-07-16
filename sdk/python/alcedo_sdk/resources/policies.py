from typing import Any, Optional

from .base import BaseResource


class PoliciesResource(BaseResource):
    async def list(self) -> dict:
        return await self._request("GET", "/api/policies")

    async def get(self, id: str) -> dict:
        return await self._request("GET", f"/api/policies/{id}")

    async def create(self, data: dict) -> dict:
        return await self._request("POST", "/api/policies", json=data)

    async def update(self, id: str, data: dict) -> dict:
        return await self._request("PUT", f"/api/policies/{id}", json=data)

    async def delete(self, id: str) -> dict:
        return await self._request("DELETE", f"/api/policies/{id}")

    async def list_permissions(self, id: str) -> dict:
        return await self._request("GET", f"/api/policies/{id}/permissions")

    async def create_permission(self, id: str, data: dict) -> dict:
        return await self._request("POST", f"/api/policies/{id}/permissions", json=data)

    async def update_permission(self, id: str, permission_id: str, data: dict) -> dict:
        return await self._request("PUT", f"/api/policies/{id}/permissions/{permission_id}", json=data)

    async def delete_permission(self, id: str, permission_id: str) -> dict:
        return await self._request("DELETE", f"/api/policies/{id}/permissions/{permission_id}")

    async def list_plugins(self, id: str) -> dict:
        return await self._request("GET", f"/api/policies/{id}/plugins")

    async def list_plugin_policies(self, plugin_slug: str) -> dict:
        return await self._request("GET", f"/api/policies/plugins/{plugin_slug}")

    async def assign_plugin_policy(self, plugin_slug: str, policy_id: str) -> dict:
        return await self._request("POST", f"/api/policies/plugins/{plugin_slug}/policies", json={"policy_id": policy_id})

    async def unassign_plugin_policy(self, plugin_slug: str, policy_id: str) -> dict:
        return await self._request("DELETE", f"/api/policies/plugins/{plugin_slug}/policies/{policy_id}")
