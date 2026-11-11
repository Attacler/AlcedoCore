from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class LogsResource(BaseResource):
    """Plugin request log resource. Maps to GET /api/platform/plugins/{slug}/logs."""

    async def list(
        self,
        slug: Optional[str] = None,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
        start_date: Optional[str] = None,
        end_date: Optional[str] = None,
        target: Optional[str] = None,
        operation_type: Optional[str] = None,
        item_id: Optional[str] = None,
        params: Optional[dict[str, Any]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Get request logs for a plugin.

        Args:
            slug: Plugin slug; defaults to the client's plugin_slug.
            limit: Max number of log entries to return
            offset: Pagination offset
            start_date: Filter by start date (ISO format)
            end_date: Filter by end date (ISO format)
            target: Filter by target
            operation_type: Filter by operation type
            item_id: Filter by item ID
            params: Extra raw query params (merged last)
        Returns: The API's data payload (list of log entry dicts).
        """
        resolved_slug = slug if slug is not None else self._plugin_slug
        query: dict[str, Any] = {}
        if limit is not None:
            query["limit"] = str(limit)
        if offset is not None:
            query["offset"] = str(offset)
        if start_date is not None:
            query["start_date"] = start_date
        if end_date is not None:
            query["end_date"] = end_date
        if target is not None:
            query["target"] = target
        if operation_type is not None:
            query["operation_type"] = operation_type
        if item_id is not None:
            query["item_id"] = item_id
        if params:
            query.update(params)
        return await self._request(
            "GET",
            f"/api/platform/plugins/{_enc(resolved_slug)}/logs",
            params=query or None,
            app=app,
            version=version,
        )
