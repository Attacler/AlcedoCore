from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class UsageResource(BaseResource):
    """Usage resource. Maps to GET /api/usage/{name}."""

    async def get(self, name: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", f"/api/usage/{_enc(name)}", app=app, version=version)

    __call__ = get
