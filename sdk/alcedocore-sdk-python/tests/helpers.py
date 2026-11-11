"""Shared test helpers for building JSend responses."""

from typing import Any, Optional


def jsend(data: Any = None) -> dict:
    """Build a JSend success envelope as returned by the core."""
    return {"status": "success", "data": data}


def jsend_error(
    message: str = "boom",
    code: Optional[int] = None,
    status: str = "error",
) -> dict:
    """Build a JSend fail/error envelope."""
    body: dict = {"status": status, "message": message}
    if code is not None:
        body["code"] = code
    return body
