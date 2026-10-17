from typing import Any, Optional

from .base import BaseResource


class AuthResource(BaseResource):
    """Platform auth resource. Maps to /api/platform/auth/*."""

    async def me(
        self,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request("GET", "/api/platform/auth/me", app=app, version=version)

    async def login(
        self,
        email: str,
        password: str,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request(
            "POST",
            "/api/platform/auth/login",
            json={"email": email, "password": password},
            app=app,
            version=version,
        )

    async def logout(
        self,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict:
        return await self._request("POST", "/api/platform/auth/logout", app=app, version=version)
