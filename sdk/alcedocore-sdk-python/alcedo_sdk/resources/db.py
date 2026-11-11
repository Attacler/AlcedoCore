import os
from typing import Any, Optional

from .base import BaseResource


class DBResource(BaseResource):
    """Plugin database query resource. Maps to POST {PLUGIN_BASE_PATH}/db/query.

    The plugin proxy is install-id keyed (``/p/{install_id}/...``), never
    slug keyed. The base path is resolved from, in order: the explicit
    ``base_path`` argument, the explicit ``install_id`` argument, then the
    ``PLUGIN_BASE_PATH`` env var the core injects into every plugin.
    """

    def _resolve_base_path(
        self,
        install_id: Optional[int] = None,
        base_path: Optional[str] = None,
    ) -> str:
        if base_path is not None:
            return base_path.rstrip("/")
        if install_id is not None:
            return f"/p/{install_id}"
        env_base = os.environ.get("PLUGIN_BASE_PATH")
        if env_base:
            return env_base.rstrip("/")
        raise ValueError(
            "No plugin base path available: pass install_id= or base_path=, "
            "or set the PLUGIN_BASE_PATH env var (injected by the core as /p/{install_id})"
        )

    async def query(
        self,
        sql: str,
        params: Optional[list] = None,
        timeout_secs: int = 30,
        max_rows: int = 100,
        install_id: Optional[int] = None,
        base_path: Optional[str] = None,
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
            install_id: Plugin install id; the proxy path becomes /p/{install_id}
            base_path: Explicit proxy base path (e.g. "/p/7"); overrides install_id
                and the PLUGIN_BASE_PATH env var.

        Returns:
            dict with keys: columns (list[str]), rows (list[list]), row_count (int),
            truncated (bool), execution_time_ms (float).

        The timeout applies to the database query execution, not the HTTP request.
        """
        base = self._resolve_base_path(install_id=install_id, base_path=base_path)
        return await self._request(
            "POST",
            f"{base}/db/query",
            json={
                "query": sql,
                "params": params or [],
                "timeout_secs": timeout_secs,
                "max_rows": max_rows,
            },
        )
