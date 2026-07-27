from typing import Optional

from .base import BaseResource


class LogsResource(BaseResource):
    """Plugin request log resource. Maps to GET /api/plugins/{slug}/logs."""

    async def list(
        self,
        limit: int = 50,
        offset: int = 0,
        start_date: Optional[str] = None,
        end_date: Optional[str] = None,
        target: Optional[str] = None,
        operation_type: Optional[str] = None,
        item_id: Optional[str] = None,
    ) -> list[dict]:
        """Get request logs for this plugin.

        Args:
            limit: Max number of log entries to return
            offset: Pagination offset
            start_date: Filter by start date (ISO format)
            end_date: Filter by end date (ISO format)
            target: Filter by target
            operation_type: Filter by operation type
            item_id: Filter by item ID
        Returns: List of log entry dicts.
        """
        params: dict[str, str] = {
            "limit": str(limit),
            "offset": str(offset),
        }
        if start_date is not None:
            params["start_date"] = start_date
        if end_date is not None:
            params["end_date"] = end_date
        if target is not None:
            params["target"] = target
        if operation_type is not None:
            params["operation_type"] = operation_type
        if item_id is not None:
            params["item_id"] = item_id
        return await self._request("GET", f"/api/plugins/{self._plugin_slug}/logs", params=params)
