from typing import Any, Optional

import httpx

from ..exceptions import (
    AlcedoApiError,
    AlcedoError,
    AuthenticationError,
    ConnectionFailedError,
    NotFoundError,
    ServerError,
    ValidationError,
)

JSEND_STATUSES = ("success", "fail", "error")


def is_jsend_envelope(body: Any) -> bool:
    """True when the parsed body is a JSend envelope ({status, data, message, code})."""
    return (
        isinstance(body, dict)
        and body.get("status") in JSEND_STATUSES
    )


class BaseResource:
    """Base class for all Alcedo SDK resource modules.

    Provides shared HTTP transport via _request() and JSend envelope handling:

    - ``{"status": "success", "data": ...}`` → the unwrapped ``data`` payload.
    - ``{"status": "fail" | "error", ...}`` → raises
      :class:`~alcedo_sdk.exceptions.AlcedoApiError` carrying the JSend
      ``message``/``code`` (mirrors the node client's ``unwrapJSend`` hook).
    - Non-JSend error responses keep the typed exception mapping
      (NotFoundError, ValidationError, ...).

    App context (``X-App``/``X-Version`` headers) is resolved per call against
    the client defaults, mirroring the node SDK's ``resolveAppHeaders``.
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

    def __init__(
        self,
        client: httpx.AsyncClient,
        plugin_slug: str = "system",
        default_app: Optional[str] = None,
        default_version: Optional[str] = None,
    ):
        self._client = client
        self._plugin_slug = plugin_slug
        self._default_app = default_app
        self._default_version = default_version

    def _resolve_app_headers(
        self,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> dict[str, str]:
        """Merge per-call app/version with the client defaults (per-call wins)."""
        headers: dict[str, str] = {}
        resolved_app = app if app is not None else self._default_app
        resolved_version = version if version is not None else self._default_version
        if resolved_app:
            headers["X-App"] = resolved_app
        if resolved_version:
            headers["X-Version"] = resolved_version
        return headers

    async def _request(
        self,
        method: str,
        path: str,
        *,
        json: Optional[Any] = None,
        params: Optional[dict[str, Any]] = None,
        timeout: Optional[float] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
        headers: Optional[dict[str, str]] = None,
    ) -> Any:
        """Make an HTTP request and return the unwrapped JSend ``data`` payload.

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
        request_headers = self._resolve_app_headers(app, version)
        if headers:
            request_headers.update(headers)
        if request_headers:
            kwargs["headers"] = request_headers

        try:
            response = await self._client.request(**kwargs)
        except httpx.HTTPError as exc:
            raise ConnectionFailedError(
                message=str(exc),
                status_code=None,
            ) from exc

        return await self._handle_response(response)

    async def _request_text(
        self,
        path: str,
        *,
        params: Optional[dict[str, Any]] = None,
        app: Optional[str] = None,
        version: Optional[str] = None,
    ) -> str:
        """Make a GET request and return the raw response text (no JSend unwrapping)."""
        kwargs: dict[str, Any] = {"method": "GET", "url": path}
        if params is not None:
            kwargs["params"] = params
        request_headers = self._resolve_app_headers(app, version)
        if request_headers:
            kwargs["headers"] = request_headers

        try:
            response = await self._client.request(**kwargs)
        except httpx.HTTPError as exc:
            raise ConnectionFailedError(
                message=str(exc),
                status_code=None,
            ) from exc

        if response.is_success:
            return response.text

        exc_class = self.EXCEPTION_MAP.get(response.status_code, AlcedoError)
        raise exc_class(
            message=response.text or response.reason_phrase,
            status_code=response.status_code,
        )

    async def _handle_response(self, response: httpx.Response) -> Any:
        """Unwrap the JSend envelope or map HTTP errors to typed exceptions."""
        if response.is_success:
            try:
                body = response.json()
            except Exception:
                return None
            if is_jsend_envelope(body):
                if body["status"] != "success":
                    raise AlcedoApiError(
                        body.get("message") or "Request failed",
                        code=body.get("code"),
                        status=body["status"],
                        http_status=response.status_code,
                    )
                return body.get("data")
            return body

        try:
            body = response.json()
        except Exception:
            body = None

        if is_jsend_envelope(body):
            raise AlcedoApiError(
                body.get("message") or "Request failed",
                code=body.get("code"),
                status=body["status"],
                http_status=response.status_code,
            )

        if body is not None and isinstance(body, dict):
            message = (
                body.get("error", {}).get("message")
                if isinstance(body.get("error"), dict)
                else None
            ) or body.get("message")
            if message is None:
                message = response.text
        else:
            message = response.text

        exc_class = self.EXCEPTION_MAP.get(
            response.status_code,
            AlcedoError,
        )

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
