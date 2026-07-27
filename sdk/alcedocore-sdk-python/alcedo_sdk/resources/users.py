from typing import Any, Optional

from .base import BaseResource


class UsersResource(BaseResource):
    async def list(self) -> dict:
        return await self._request("GET", "/api/users")

    async def get(self, id: str) -> dict:
        return await self._request("GET", f"/api/users/{id}")

    async def create(self, data: dict) -> dict:
        return await self._request("POST", "/api/users", json=data)

    async def update(self, id: str, data: dict) -> dict:
        return await self._request("PUT", f"/api/users/{id}", json=data)

    async def delete(self, id: str) -> dict:
        return await self._request("DELETE", f"/api/users/{id}")

    async def list_roles(self, id: str) -> dict:
        return await self._request("GET", f"/api/users/{id}/roles")

    async def assign_role(self, id: str, role_id: str) -> dict:
        return await self._request("POST", f"/api/users/{id}/roles", json={"role_id": role_id})

    async def remove_role(self, id: str, role_id: str) -> dict:
        return await self._request("DELETE", f"/api/users/{id}/roles/{role_id}")
