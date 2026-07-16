from typing import Any, Optional

import httpx

from ..exceptions import (
    AlcedoError,
    AuthenticationError,
    ConnectionFailedError,
    NotFoundError,
    ServerError,
    ValidationError,
)


class BaseResource:
    """Base class for all Alcedo SDK resource modules.

    Provides shared HTTP transport via _request() and _handle_response().
    Subclasses inherit the EXCEPTION_MAP for status code mapping.
    """

    EXCEPTION_MAP: dict[int, type[Exception]] = {
        400: ValidationError,
        401: AuthenticationError,
        403: AuthenticationError,
        404: NotFoundError,
        422: ValidationError,
        500: ServerError,
        502: ServerError,
        503: ServerError,
    }

    def __init__(self, client: httpx.AsyncClient, plugin_slug: str = "system"):
        self._client = client
        self._plugin_slug = plugin_slug

    async def _request(
        self,
        method: str,
        path: str,
        *,
        json: Optional[Any] = None,
        params: Optional[dict[str, Any]] = None,
        timeout: Optional[float] = None,
    ) -> Any:
        """Make an HTTP request and return the parsed JSON response.

        Only non-None optional kwargs are passed to the client
        to avoid httpx warnings on some versions.
        """
        kwargs: dict[str, Any] = {"method": method, "url": path}
        if json is not None:
            kwargs["json"] = json
        if params is not None:
            kwargs["params"] = params
        if timeout is not None:
            kwargs["timeout"] = timeout

        try:
            response = await self._client.request(**kwargs)
        except httpx.HTTPError as exc:
            raise ConnectionFailedError(
                message=str(exc),
                status_code=None,
            ) from exc

        return await self._handle_response(response)

    async def _handle_response(self, response: httpx.Response) -> Any:
        """Map HTTP response to typed result or raise typed exception."""
        if response.is_success:
            try:
                return response.json()
            except Exception:
                return None

        exc_class = self.EXCEPTION_MAP.get(
            response.status_code,
            AlcedoError,
        )

        try:
            body = response.json()
            message = (
                body.get("error", {}).get("message")
                or body.get("message")
                or response.text
            )
        except Exception:
            message = response.text

        # Special case for NotFoundError: pass key=None so callers can
        # catch NotFoundError and check key separately if needed
        if exc_class is NotFoundError:
            raise NotFoundError(
                message=message,
                status_code=response.status_code,
                key=None,
            )

        raise exc_class(
            message=message,
            status_code=response.status_code,
        )
