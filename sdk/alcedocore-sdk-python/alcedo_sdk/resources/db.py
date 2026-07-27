from typing import Any, Optional

from .base import BaseResource


class DBResource(BaseResource):
    """Plugin database query resource. Maps to POST /p/{slug}/db/query."""

    async def query(
        self,
        sql: str,
        params: Optional[list[Any]] = None,
        timeout_secs: int = 30,
        max_rows: int = 100,
    ) -> dict:
        """Execute a read-only SQL query against the plugin's database schema.

        WARNING: Never use string formatting or f-strings to interpolate user
        input into the SQL query. Always use the `params` parameter for dynamic
        values to prevent SQL injection.

        Args:
            sql: Parameterized SQL query string (e.g., "SELECT * FROM items WHERE id = $1")
            params: Query parameters to safely substitute into the SQL
            timeout_secs: Database query execution timeout
            max_rows: Maximum number of rows to return

        Returns:
            dict with keys: columns (list[str]), rows (list[list]), row_count (int),
            truncated (bool), execution_time_ms (float).

        The timeout applies to the database query execution, not the HTTP request.
        """
        return await self._request(
            "POST",
            f"/p/{self._plugin_slug}/db/query",
            json={
                "query": sql,
                "params": params or [],
                "timeout_secs": timeout_secs,
                "max_rows": max_rows,
            },
        )
