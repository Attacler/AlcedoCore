from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


def _coll(name: str, suffix: str = "") -> str:
    return f"/api/app/collections/{_enc(name)}{suffix}"


class CollectionsResource(BaseResource):
    """Collections resource. Maps to /api/app/collections (app-scoped).

    Mirrors the node SDK's ``collections`` resource: CRUD, layouts,
    layout-scoped sections, and saved views.
    """

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/app/collections", app=app, version=version)

    async def get(self, name: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _coll(name), app=app, version=version)

    async def create(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("POST", "/api/app/collections", json=data, app=app, version=version)

    async def update(
        self,
        name: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request("PUT", _coll(name), json=data, app=app, version=version)

    async def delete(self, name: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("DELETE", _coll(name), app=app, version=version)

    # --- layouts ---

    async def list_layouts(self, name: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _coll(name, "/layouts"), app=app, version=version)

    async def create_layout(
        self,
        name: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST", _coll(name, "/layouts"), json=data, app=app, version=version
        )

    async def update_layout(
        self,
        name: str,
        layout_id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT", _coll(name, f"/layouts/{_enc(layout_id)}"), json=data, app=app, version=version
        )

    async def delete_layout(
        self,
        name: str,
        layout_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE", _coll(name, f"/layouts/{_enc(layout_id)}"), app=app, version=version
        )

    async def get_layout_roles(
        self,
        name: str,
        layout_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", _coll(name, f"/layouts/{_enc(layout_id)}/roles"), app=app, version=version
        )

    async def set_layout_roles(
        self,
        name: str,
        layout_id: str,
        role_ids: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT",
            _coll(name, f"/layouts/{_enc(layout_id)}/roles"),
            json={"role_ids": role_ids},
            app=app,
            version=version,
        )

    async def get_resolved_layout(
        self,
        name: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request("GET", _coll(name, "/layout"), app=app, version=version)

    # --- layout-scoped sections ---

    async def list_layout_sections(
        self,
        name: str,
        layout_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", _coll(name, f"/layouts/{_enc(layout_id)}/sections"), app=app, version=version
        )

    async def create_layout_section(
        self,
        name: str,
        layout_id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            _coll(name, f"/layouts/{_enc(layout_id)}/sections"),
            json=data,
            app=app,
            version=version,
        )

    async def update_layout_section(
        self,
        name: str,
        layout_id: str,
        section_id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT",
            _coll(name, f"/layouts/{_enc(layout_id)}/sections/{_enc(section_id)}"),
            json=data,
            app=app,
            version=version,
        )

    async def delete_layout_section(
        self,
        name: str,
        layout_id: str,
        section_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE",
            _coll(name, f"/layouts/{_enc(layout_id)}/sections/{_enc(section_id)}"),
            app=app,
            version=version,
        )

    async def batch_reorder_sections(
        self,
        name: str,
        layout_id: str,
        sections: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PATCH",
            _coll(name, f"/layouts/{_enc(layout_id)}/sections"),
            json={"sections": sections},
            app=app,
            version=version,
        )

    # --- saved views ---

    async def list_views(self, name: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _coll(name, "/views"), app=app, version=version)

    async def create_view(
        self,
        name: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request("POST", _coll(name, "/views"), json=data, app=app, version=version)

    async def update_view(
        self,
        name: str,
        view_id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT", _coll(name, f"/views/{_enc(view_id)}"), json=data, app=app, version=version
        )

    async def delete_view(
        self,
        name: str,
        view_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE", _coll(name, f"/views/{_enc(view_id)}"), app=app, version=version
        )

    async def set_default_view(
        self,
        name: str,
        view_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PUT", _coll(name, f"/views/{_enc(view_id)}/default"), app=app, version=version
        )

    async def get_create_policy(
        self,
        name: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request("GET", _coll(name, "/$create"), app=app, version=version)
