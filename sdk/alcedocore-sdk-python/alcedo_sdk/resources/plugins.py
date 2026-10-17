from typing import Any, Optional
from urllib.parse import quote

from .base import BaseResource


def _enc(value: Any) -> str:
    return quote(str(value), safe="")


def _slug_path(slug: str, suffix: str = "") -> str:
    return f"/api/platform/plugins/{_enc(slug)}{suffix}"


def _install_path(slug: str, app_version_id: int, suffix: str = "") -> str:
    return _slug_path(slug, f"/installs/{app_version_id}{suffix}")


def _install_id_path(install_id: int, suffix: str = "") -> str:
    # Runtime/instances are keyed by the install's own id, not by slug + version.
    return f"/api/platform/plugins/installs/{_enc(install_id)}{suffix}"


class PluginsResource(BaseResource):
    """Plugins resource. Slug + install-scoped routes under /api/platform/plugins.

    Mirrors the node SDK's ``plugins`` resource.
    """

    async def list(self, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", "/api/platform/plugins", app=app, version=version)

    async def get(self, slug: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _slug_path(slug), app=app, version=version)

    async def update(
        self,
        slug: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request("PUT", _slug_path(slug), json=data, app=app, version=version)

    async def delete(self, slug: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("DELETE", _slug_path(slug), app=app, version=version)

    # --- deploy ---

    async def deploy(self, data: dict, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "POST", "/api/platform/plugins/deploy", json=data, app=app, version=version
        )

    async def preview(
        self,
        image: str,
        registry_id: Optional[int] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            "/api/platform/plugins/preview",
            json={"image": image, "registry_id": registry_id},
            app=app,
            version=version,
        )

    # --- installs (plugin x app-version) ---

    async def uninstall(
        self,
        slug: str,
        app_version_id: int,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "DELETE", _install_path(slug, app_version_id), app=app, version=version
        )

    async def enable(
        self,
        slug: str,
        app_version_id: int,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST", _install_path(slug, app_version_id, "/enable"), app=app, version=version
        )

    async def disable(
        self,
        slug: str,
        app_version_id: int,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST", _install_path(slug, app_version_id, "/disable"), app=app, version=version
        )

    async def get_settings(
        self,
        slug: str,
        app_version_id: int,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", _install_path(slug, app_version_id, "/settings"), app=app, version=version
        )

    async def update_settings(
        self,
        slug: str,
        app_version_id: int,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "PATCH",
            _install_path(slug, app_version_id, "/settings"),
            json=data,
            app=app,
            version=version,
        )

    async def get_scopes(
        self,
        slug: str,
        app_version_id: int,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", _install_path(slug, app_version_id, "/scopes"), app=app, version=version
        )

    async def update_scopes(
        self,
        slug: str,
        app_version_id: int,
        scopes: list,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            _install_path(slug, app_version_id, "/scopes"),
            json={"scopes": scopes},
            app=app,
            version=version,
        )

    # --- runtime (keyed by the install's own id) ---

    async def runtime_info(
        self,
        install_id: int,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", _install_id_path(install_id, "/runtime"), app=app, version=version
        )

    async def instances(
        self,
        install_id: int,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", _install_id_path(install_id, "/instances"), app=app, version=version
        )

    async def instance_detail(
        self,
        install_id: int,
        task_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET",
            _install_id_path(install_id, f"/instances/{_enc(task_id)}"),
            app=app,
            version=version,
        )

    async def instance_stats(
        self,
        install_id: int,
        task_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET",
            _install_id_path(install_id, f"/instances/{_enc(task_id)}/stats"),
            app=app,
            version=version,
        )

    async def instance_logs(
        self,
        install_id: int,
        task_id: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET",
            _install_id_path(install_id, f"/instances/{_enc(task_id)}/logs"),
            app=app,
            version=version,
        )

    # --- slug-scoped subresources ---

    async def versions(self, slug: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", _slug_path(slug, "/versions"), app=app, version=version
        )

    async def request_logs(
        self,
        slug: str,
        params: Optional[dict[str, Any]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "GET", _slug_path(slug, "/logs"), params=params, app=app, version=version
        )

    async def docs(self, slug: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _slug_path(slug, "/docs"), app=app, version=version)

    async def doc_content(
        self,
        slug: str,
        doc_path: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> str:
        clean = doc_path
        if clean.startswith("docs/"):
            clean = clean[5:]
        elif clean.startswith("./"):
            clean = clean[2:]
        return await self._request_text(_slug_path(slug, f"/docs/{clean}"), app=app, version=version)

    async def schema(self, slug: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _slug_path(slug, "/schema"), app=app, version=version)

    async def pages(self, slug: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request("GET", _slug_path(slug, "/pages"), app=app, version=version)

    async def assets(self, slug: str, app: Optional[str] = None, version: Optional[str] = None) -> Any:
        return await self._request(
            "GET", _slug_path(slug, "/pages/assets"), app=app, version=version
        )

    async def scale(
        self,
        slug: str,
        data: dict,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST", _slug_path(slug, "/scale"), json=data, app=app, version=version
        )

    async def restart(
        self,
        slug: str,
        container_id: Optional[str] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> Any:
        return await self._request(
            "POST",
            _slug_path(slug, "/restart"),
            json={"deployment_id": container_id},
            app=app,
            version=version,
        )
