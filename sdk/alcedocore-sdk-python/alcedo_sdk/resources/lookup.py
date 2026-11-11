from typing import Any, Optional

from .base import BaseResource


class LookupResource(BaseResource):
    """Lookup resource. Maps to /api/app/lookup/* (app-scoped)."""

    async def users(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        """App-scoped user options for the ``user`` field input.

        Users are not exposed through the generic items API; this endpoint
        enforces the users collection's read policy.
        """
        return await self._request("GET", "/api/app/lookup/users", app=app, version=version)
