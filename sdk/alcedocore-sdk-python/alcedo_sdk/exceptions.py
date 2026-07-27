from typing import Any, Optional


class AlcedoError(Exception):
    """Base exception for all Alcedo SDK errors."""

    def __init__(self, message: str, status_code: Optional[int] = None):
        super().__init__(message)
        self.message = message
        self.status_code = status_code

    def __str__(self) -> str:
        if self.status_code is not None:
            return f"[{self.status_code}] {self.message}"
        return self.message


class ConnectionFailedError(AlcedoError):
    """Network or connection failure (includes httpx TransportError).

    This is the SDK's own exception type, distinct from Python's built-in
    ``ConnectionError``. Use this exception to catch transport-level failures
    from the Alcedo SDK.
    """

    def __init__(self, message: str, status_code: Optional[int] = None):
        super().__init__(message, status_code)
        self.original: Optional[Exception] = None


class NotFoundError(AlcedoError):
    """Resource not found (HTTP 404)."""

    def __init__(
        self,
        message: str,
        status_code: Optional[int] = 404,
        key: Optional[str] = None,
    ):
        super().__init__(message, status_code)
        self.key = key


class ValidationError(AlcedoError):
    """Request validation failure (HTTP 400/422)."""

    def __init__(
        self,
        message: str,
        status_code: Optional[int] = 400,
        details: Optional[Any] = None,
    ):
        super().__init__(message, status_code)
        self.details = details


class AuthenticationError(AlcedoError):
    """Authentication/authorization failure (HTTP 401/403)."""
    pass


class ServerError(AlcedoError):
    """Server-side error (HTTP 5xx)."""
    pass


# Backward-compatible alias: KVStoreError -> AlcedoError
KVStoreError = AlcedoError


class KeyNotFoundError(NotFoundError):
    """DEPRECATED: Use NotFoundError instead.

    Fires a DeprecationWarning on instantiation.
    Kept for backward compatibility with existing code.
    """

    def __init__(self, key: str):
        import warnings
        warnings.warn(
            "KeyNotFoundError is deprecated, use NotFoundError instead",
            DeprecationWarning,
            stacklevel=2,
        )
        super().__init__(message=f"Key not found: {key}", status_code=404, key=key)


# Backward-compatible alias: ConnectionError -> ConnectionFailedError
# Note: Not exported in __all__ to avoid shadowing Python's built-in ConnectionError
ConnectionError = ConnectionFailedError
