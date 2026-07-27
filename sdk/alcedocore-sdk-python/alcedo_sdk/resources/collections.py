from typing import Any, Optional

from .base import BaseResource


class CollectionsResource(BaseResource):
    async def list(self) -> dict:
        return await self._request("GET", "/api/collections")

    async def get(self, name: str) -> dict:
        return await self._request("GET", f"/api/collections/{name}")

    async def create(self, name: str, data: dict) -> dict:
        return await self._request("POST", f"/api/collections/{name}", json=data)

    async def update(self, name: str, data: dict) -> dict:
        return await self._request("PUT", f"/api/collections/{name}", json=data)

    async def delete(self, name: str) -> dict:
        return await self._request("DELETE", f"/api/collections/{name}")

    async def list_sections(self, name: str) -> dict:
        return await self._request("GET", f"/api/collections/{name}/sections")

    async def create_section(self, name: str, data: dict) -> dict:
        return await self._request("POST", f"/api/collections/{name}/sections", json=data)

    async def update_section(self, name: str, section_id: str, data: dict) -> dict:
        return await self._request("PUT", f"/api/collections/{name}/sections/{section_id}", json=data)

    async def delete_section(self, name: str, section_id: str) -> dict:
        return await self._request("DELETE", f"/api/collections/{name}/sections/{section_id}")

    async def list_views(self, name: str) -> dict:
        return await self._request("GET", f"/api/collections/{name}/views")

    async def create_view(self, name: str, data: dict) -> dict:
        return await self._request("POST", f"/api/collections/{name}/views", json=data)

    async def update_view(self, name: str, view_id: str, data: dict) -> dict:
        return await self._request("PUT", f"/api/collections/{name}/views/{view_id}", json=data)

    async def delete_view(self, name: str, view_id: str) -> dict:
        return await self._request("DELETE", f"/api/collections/{name}/views/{view_id}")

    async def set_default_view(self, name: str, view_id: str) -> dict:
        return await self._request("PUT", f"/api/collections/{name}/views/{view_id}/default")
