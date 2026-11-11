import json
from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class ItemsResource(BaseResource):
    """Collection items. Maps to /api/app/items/{name} (app-scoped).

    Mirrors the node SDK's ``items`` resource, including the special case that
    listing the ``users`` collection goes through /api/platform/users.
    """

    async def list(
        self,
        name: str,
        params: Optional[dict[str, str]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        path = (
            "/api/platform/users"
            if name == "users"
            else f"/api/app/items/{_enc(name)}"
        )
        return await self._request("GET", path, params=params, app=app, version=version)

    async def create(
        self,
        name: str,
        data: Any,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST", f"/api/app/items/{_enc(name)}", json=data, app=app, version=version
        )

    async def update(
        self,
        name: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Bulk update: data is {"filter": ..., "update": ...}.

        The core takes the filter as a URL-encoded query param and the update
        map as the body; it answers 400 to a body-carried filter and registers
        the route as PATCH (PUT is a 405).
        """
        filter_ = data.get("filter") or {}
        update = data.get("update") or {k: v for k, v in data.items() if k != "filter"}
        return await self._request(
            "PATCH",
            f"/api/app/items/{_enc(name)}",
            json=update,
            params={"filter": json.dumps(filter_)},
            app=app,
            version=version,
        )

    async def delete(
        self,
        name: str,
        data: Optional[dict] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Bulk delete: body is {"filter": ...} or {"pk_values": [...]}."""
        return await self._request(
            "DELETE", f"/api/app/items/{_enc(name)}", json=data, app=app, version=version
        )

    async def get(
        self,
        name: str,
        id: str,
        params: Optional[dict[str, str]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET",
            f"/api/app/items/{_enc(name)}/{_enc(id)}",
            params=params,
            app=app,
            version=version,
        )

    async def patch(
        self,
        name: str,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PATCH",
            f"/api/app/items/{_enc(name)}/{_enc(id)}",
            json=data,
            app=app,
            version=version,
        )

    async def query(
        self,
        name: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """List with a structured query dict.

        Supports filter/limit/offset/fields/sort plus group-by/aggregate,
        emitted as repeated query params on GET /{collection}:
        ``groupBy[]=field`` (``data["groupBy"]``: str or list) and
        ``aggregate[fn]=field`` (``data["aggregate"]``: {fn: field | [fields]}).
        """
        params: list[tuple[str, str]] = []
        if data.get("filter"):
            params.append(("filter", json.dumps(data["filter"])))
        if data.get("limit"):
            params.append(("limit", str(data["limit"])))
        if data.get("offset"):
            params.append(("offset", str(data["offset"])))
        if data.get("fields"):
            params.append(("fields", ",".join(data["fields"])))
        if data.get("groupBy"):
            group_by = data["groupBy"]
            for field in group_by if isinstance(group_by, list) else [group_by]:
                params.append(("groupBy[]", str(field)))
        if data.get("aggregate"):
            for fn, fields in data["aggregate"].items():
                for field in fields if isinstance(fields, list) else [fields]:
                    params.append((f"aggregate[{fn}]", str(field)))
        if data.get("sort"):
            sort = data["sort"]
            if isinstance(sort, list) and sort:
                params.append(("sort", sort[0]["field"]))
                order = sort[0].get("order") or sort[0].get("direction")
                if order:
                    params.append(("order", order))
            elif isinstance(sort, str):
                params.append(("sort", sort))
        return await self._request(
            "GET", f"/api/app/items/{_enc(name)}", params=params, app=app, version=version
        )

    async def references(
        self,
        name: str,
        id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", f"/api/app/items/{_enc(name)}/{_enc(id)}/references", app=app, version=version
        )

    async def delete_permissions(
        self,
        name: str,
        pk_values: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Per-pk "may I delete this row" flags."""
        return await self._request(
            "POST",
            f"/api/app/items/{_enc(name)}/$delete",
            json={"pk_values": pk_values},
            app=app,
            version=version,
        )

    async def update_permissions(
        self,
        name: str,
        pk_values: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Per-pk "may I edit this row" flags (from the update rules' row filters)."""
        return await self._request(
            "POST",
            f"/api/app/items/{_enc(name)}/$update",
            json={"pk_values": pk_values},
            app=app,
            version=version,
        )

    async def permissions(
        self,
        name: str,
        item: dict,
        action: str = "create",
        pk: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Resolve what the caller may do with a candidate item.

        ``update`` judges a delta against an existing row, so it also requires ``pk``.
        """
        body: dict[str, Any] = {"action": action, "item": item}
        if pk is not None:
            body["pk"] = pk
        return await self._request(
            "POST", f"/api/app/items/{_enc(name)}/$permissions", json=body, app=app, version=version
        )
