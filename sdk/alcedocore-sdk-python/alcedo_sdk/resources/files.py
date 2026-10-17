import mimetypes
import os
from typing import Any, Optional
from urllib.parse import quote, urlencode

import httpx

from ..exceptions import AlcedoError
from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


def resolve_download_url(
    base_url: str,
    default_app: Optional[str],
    default_version: Optional[str],
    file: Any,
    app: Optional[str] = None,
    version: Optional[str] = None,
) -> str:
    """Build the `<img src>` / download path for a file.

    Prefers the server's ``download_url`` when present (it already carries the
    owning app/version, so cross-app relational images work); otherwise builds
    the same path from the id + app/version.
    """
    if isinstance(file, dict) and file.get("download_url"):
        return file["download_url"]
    file_id = file if isinstance(file, str) else (file.get("id") if isinstance(file, dict) else None)
    if not file_id:
        return ""

    resolved_app = app if app is not None else default_app
    resolved_version = version if version is not None else default_version
    params: dict[str, str] = {}
    if resolved_app:
        params["ac_app"] = resolved_app
    if resolved_version:
        params["ac_version"] = resolved_version
    query = urlencode(params)

    root = base_url.rstrip("/")
    return f"{root}/api/app/files/{_enc(file_id)}/download" + (f"?{query}" if query else "")


class FoldersResource(BaseResource):
    """File folders. Maps to /api/app/files/folders."""

    async def create(
        self,
        name: str,
        parent_id: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        body: dict[str, Any] = {"name": name}
        if parent_id is not None:
            body["parent_id"] = parent_id
        return await self._request(
            "POST", "/api/app/files/folders", json=body, app=app, version=version
        )

    async def list(
        self,
        parent_id: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        params = {"parent_id": parent_id} if parent_id else None
        return await self._request(
            "GET", "/api/app/files/folders", params=params, app=app, version=version
        )

    async def get(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/app/files/folders/{_enc(id)}", app=app, version=version
        )

    async def update(
        self,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PATCH", f"/api/app/files/folders/{_enc(id)}", json=data, app=app, version=version
        )

    async def delete(
        self,
        id: str,
        recursive: bool = False,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        params = {"recursive": "true"} if recursive else None
        return await self._request(
            "DELETE", f"/api/app/files/folders/{_enc(id)}", params=params, app=app, version=version
        )


class FilesResource(BaseResource):
    """File upload/download/list/delete/update operations. Maps to /api/app/files."""

    def __init__(
        self,
        client: httpx.AsyncClient,
        plugin_slug: str = "system",
        default_app: Optional[str] = None,
        default_version: Optional[str] = None,
        base_url: str = "",
    ):
        super().__init__(client, plugin_slug, default_app, default_version)
        self._base_url = base_url
        self.folders = FoldersResource(
            client, plugin_slug, default_app, default_version
        )

    async def upload(
        self,
        file: Any,
        filename: Optional[str] = None,
        collection_name: Optional[str] = None,
        item_id: Optional[str] = None,
        field_name: Optional[str] = None,
        folder_id: Optional[str] = None,
        overwrite: bool = False,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        """Upload a file as multipart form data.

        ``file`` may be a filesystem path (str), raw bytes, or a file-like
        object. ``filename`` is required unless ``file`` is a path.
        """
        if isinstance(file, str):
            filename = filename or os.path.basename(file)
            mime_type, _ = mimetypes.guess_type(file)
            file_obj = open(file, "rb")
            close = True
        elif hasattr(file, "read"):
            file_obj = file
            close = False
            mime_type = None
        else:
            file_obj = file
            close = False
            mime_type = None
            filename = filename or "upload"

        try:
            files = {"file": (filename, file_obj, mime_type or "application/octet-stream")}
            data: dict[str, str] = {}
            if collection_name:
                data["collection_name"] = collection_name
            if item_id:
                data["item_id"] = item_id
            if field_name:
                data["field_name"] = field_name
            if folder_id:
                data["folder_id"] = folder_id
            if overwrite:
                data["overwrite"] = "true"

            headers = self._resolve_app_headers(app, version)
            response = await self._client.post(
                "/api/app/files/upload",
                files=files,
                data=data or None,
                headers=headers or None,
            )
            return await self._handle_response(response)
        finally:
            if close:
                file_obj.close()

    def download_url(self, file: Any, app: Optional[str] = None, version: Optional[str] = None) -> str:
        """The URL to use as an `<img src>` (or a download link) for a file.

        Pass the metadata object if you have it, or just its id.
        """
        return resolve_download_url(
            self._base_url, self._default_app, self._default_version, file, app, version
        )

    async def download(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> bytes:
        headers = self._resolve_app_headers(app, version)
        response = await self._client.get(
            f"/api/app/files/{_enc(id)}/download",
            headers=headers or None,
        )
        if not response.is_success:
            raise AlcedoError(
                f"Failed to download file {id}",
                status_code=response.status_code,
            )
        return response.content

    async def get(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", f"/api/app/files/{_enc(id)}", app=app, version=version
        )

    async def list(
        self,
        params: Optional[dict[str, Any]] = None,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
        search: Optional[str] = None,
        mime_type: Optional[str] = None,
        folder_id: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        query: dict[str, Any] = dict(params or {})
        if limit is not None:
            query["limit"] = limit
        if offset is not None:
            query["offset"] = offset
        if search is not None:
            query["search"] = search
        if mime_type is not None:
            query["mime_type"] = mime_type
        if folder_id is not None:
            query["folder_id"] = folder_id
        return await self._request(
            "GET", "/api/app/files", params=query or None, app=app, version=version
        )

    async def update(
        self,
        id: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PATCH", f"/api/app/files/{_enc(id)}", json=data, app=app, version=version
        )

    async def delete(self, id: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "DELETE", f"/api/app/files/{_enc(id)}", app=app, version=version
        )

    async def batch_delete(
        self,
        ids: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST", "/api/app/files/batch/delete", json={"ids": ids}, app=app, version=version
        )
