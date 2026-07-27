import mimetypes
import os
from typing import Any, Optional

from .base import BaseResource


class FilesResource(BaseResource):
    """File upload/download/list/delete/update operations. Maps to /api/files endpoints."""

    async def upload(
        self,
        file_path: str,
        collection_name: Optional[str] = None,
        item_id: Optional[str] = None,
        field_name: Optional[str] = None,
    ) -> dict[str, Any]:
        filename = os.path.basename(file_path)
        mime_type, _ = mimetypes.guess_type(file_path)

        with open(file_path, "rb") as f:
            files = {"file": (filename, f, mime_type or "application/octet-stream")}
            params = {}
            if collection_name:
                params["collection_name"] = collection_name
            if item_id:
                params["item_id"] = item_id
            if field_name:
                params["field_name"] = field_name

            response = await self._client.post(
                "/api/files/upload",
                files=files,
                params=params,
            )
            return await self._handle_response(response)

    async def download(self, file_id: str) -> bytes:
        response = await self._client.get(
            f"/api/files/{file_id}/download",
        )
        response.raise_for_status()
        return response.content

    async def get(self, file_id: str) -> dict[str, Any]:
        return await self._request("GET", f"/api/files/{file_id}")

    async def list(
        self,
        search: Optional[str] = None,
        mime_type: Optional[str] = None,
        limit: int = 50,
        offset: int = 0,
    ) -> dict[str, Any]:
        params = {"limit": limit, "offset": offset}
        if search:
            params["search"] = search
        if mime_type:
            params["mime_type"] = mime_type
        return await self._request("GET", "/api/files", params=params)

    async def delete(self, file_id: str) -> None:
        await self._request("DELETE", f"/api/files/{file_id}")

    async def update(
        self,
        file_id: str,
        alt_text: Optional[str] = None,
        filename: Optional[str] = None,
    ) -> dict[str, Any]:
        body: dict[str, Any] = {}
        if alt_text is not None:
            body["alt_text"] = alt_text
        if filename is not None:
            body["filename"] = filename
        return await self._request("PATCH", f"/api/files/{file_id}", json=body)
