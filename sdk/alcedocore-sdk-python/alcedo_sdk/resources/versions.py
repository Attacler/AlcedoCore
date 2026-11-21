from typing import Any, List, Optional, Union
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


class VersionsResource(BaseResource):
    """Versions resource. Maps to /api/platform/versions."""

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/platform/versions", app=app, version=version)

    async def create(
        self,
        version_name: str,
        data: Optional[dict] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        body = data if data is not None else {"version_name": version_name}
        return await self._request(
            "POST", "/api/platform/versions", json=body, app=app, version=version
        )

    async def remove(self, id: int, app: Optional[str] = None, version: Optional[str] = None) -> dict:
        return await self._request(
            "DELETE", f"/api/platform/versions/{_enc(id)}", app=app, version=version
        )

    async def deploy_version(
        self,
        id: int,
        dry_run: bool = False,
        on_conflict: str = "fail",
        edit_ids: Optional[List[Union[int, str]]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        """Deploy a version to production: `POST /api/platform/versions/{id}/deploy`.

        ``on_conflict`` is one of ``"fail"`` (default), ``"skip"`` or
        ``"overwrite"``; ``edit_ids=None`` deploys all edits of the version,
        otherwise only the given entries are deployed — numeric entries select
        log-row ids, string entries select ``app_created`` edits by app api_name.
        A conflict with ``on_conflict="fail"`` comes back as HTTP 409 with a
        JSend ``fail`` envelope (``data.conflicts``) and raises
        :class:`AlcedoApiError` carrying the ``data`` payload.
        """
        body = {"dry_run": dry_run, "on_conflict": on_conflict, "edit_ids": edit_ids}
        return await self._request(
            "POST",
            f"/api/platform/versions/{_enc(id)}/deploy",
            json=body,
            app=app,
            version=version,
        )

    async def revert_version(
        self,
        id: int,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        """Revert a version's deployed changes: `POST /api/platform/versions/{id}/revert`.

        Applies the inverse of each recorded edit (from the version's log
        rows) in reverse order. No data backup exists — the response carries a
        ``warning`` saying so. Returns the JSend ``data`` payload with
        ``reverted`` / ``skipped`` / ``warning``.
        """
        return await self._request(
            "POST",
            f"/api/platform/versions/{_enc(id)}/revert",
            app=app,
            version=version,
        )
