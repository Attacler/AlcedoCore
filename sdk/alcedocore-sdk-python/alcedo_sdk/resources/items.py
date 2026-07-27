import json
from typing import Any, Optional

from .base import BaseResource


class ItemsResource(BaseResource):
    """Collection items CRUD operations. Maps to /api/items/{name} endpoints."""

    async def list(
        self,
        name: str,
        params: Optional[dict[str, str]] = None,
    ) -> dict:
        return await self._request("GET", f"/api/items/{name}", params=params)

    async def create(
        self,
        name: str,
        data: Any,
    ) -> dict:
        return await self._request("POST", f"/api/items/{name}", json=data)

    async def update(
        self,
        name: str,
        data: dict,
    ) -> dict:
        return await self._request("PUT", f"/api/items/{name}", json=data)

    async def delete(
        self,
        name: str,
        data: dict,
    ) -> dict:
        return await self._request("DELETE", f"/api/items/{name}", json=data)

    async def get(
        self,
        name: str,
        id: str,
        params: Optional[dict[str, str]] = None,
    ) -> dict:
        return await self._request("GET", f"/api/items/{name}/{id}", params=params)

    async def patch(
        self,
        name: str,
        id: str,
        data: dict,
    ) -> dict:
        return await self._request("PATCH", f"/api/items/{name}/{id}", json=data)

    async def query(
        self,
        name: str,
        data: dict,
    ) -> dict:
        params = {}
        if "filter" in data:
            params["filter"] = json.dumps(data["filter"])
        if "limit" in data:
            params["limit"] = str(data["limit"])
        if "offset" in data:
            params["offset"] = str(data["offset"])
        if "fields" in data:
            params["fields"] = ",".join(data["fields"])
        if "sort" in data and data["sort"]:
            params["sort"] = data["sort"][0]["field"]
            params["order"] = data["sort"][0].get("order") or data["sort"][0].get("direction", "asc")
        return await self._request("GET", f"/api/items/{name}", params=params)

    async def grouped(
        self,
        name: str,
        data: dict,
    ) -> dict:
        return await self._request("POST", f"/api/items/{name}/grouped", json=data)

    async def references(
        self,
        name: str,
        id: str,
    ) -> dict:
        return await self._request("GET", f"/api/items/{name}/{id}/references")
