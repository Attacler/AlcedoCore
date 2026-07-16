from .base import BaseResource


class AuthResource(BaseResource):
    async def me(self) -> dict:
        return await self._request("GET", "/api/auth/me")

    async def login(self, email: str, password: str) -> dict:
        return await self._request("POST", "/api/auth/login", json={"email": email, "password": password})

    async def logout(self) -> dict:
        return await self._request("POST", "/api/auth/logout")
