from __future__ import annotations

from typing import Any, Optional

from .base import BaseResource


class RolesResource(BaseResource):
    async def list(self) -> dict:
        return await self._request("GET", "/api/roles")

    async def get(self, id: str) -> dict:
        return await self._request("GET", f"/api/roles/{id}")

    async def create(self, data: dict) -> dict:
        return await self._request("POST", "/api/roles", json=data)

    async def update(self, id: str, data: dict) -> dict:
        return await self._request("PUT", f"/api/roles/{id}", json=data)

    async def delete(self, id: str) -> dict:
        return await self._request("DELETE", f"/api/roles/{id}")

    async def list_scopes(self, id: str) -> dict:
        return await self._request("GET", f"/api/roles/{id}/scopes")

    async def update_scopes(self, id: str, scopes: list[str]) -> dict:
        return await self._request("PUT", f"/api/roles/{id}/scopes", json={"scopes": scopes})

    async def list_policies(self, id: str) -> dict:
        return await self._request("GET", f"/api/roles/{id}/policies")

    async def assign_policy(self, id: str, policy_id: str) -> dict:
        return await self._request("POST", f"/api/roles/{id}/policies", json={"policy_id": policy_id})

    async def remove_policy(self, id: str, policy_id: str) -> dict:
        return await self._request("DELETE", f"/api/roles/{id}/policies/{policy_id}")
