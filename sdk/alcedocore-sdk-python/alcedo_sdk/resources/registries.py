from typing import Any, Optional

from .base import BaseResource


class RegistriesResource(BaseResource):
    async def list(self) -> dict:
        return await self._request("GET", "/api/registries")

    async def get(self, id: str) -> dict:
        return await self._request("GET", f"/api/registries/{id}")

    async def create(self, data: dict) -> dict:
        return await self._request("POST", "/api/registries", json=data)

    async def update(self, id: str, data: dict) -> dict:
        return await self._request("PUT", f"/api/registries/{id}", json=data)

    async def delete(self, id: str) -> dict:
        return await self._request("DELETE", f"/api/registries/{id}")

    async def health_check(self, id: str) -> dict:
        return await self._request("GET", f"/api/registries/{id}/health")

    async def health_check_url(self, url: str) -> dict:
        return await self._request("POST", "/api/registries/health-check", json={"url": url})

    async def images(self, id: str) -> dict:
        return await self._request("GET", f"/api/registries/{id}/images")
