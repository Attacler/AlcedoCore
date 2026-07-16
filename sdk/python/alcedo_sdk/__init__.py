"""Alcedo Python SDK — unified typed client for Alcedo Plugin Core."""

# Primary API
from .client import AlcedoClient

# Resource modules
from .resources import (
    KVResource,
    DBResource,
    SettingsResource,
    MigrationsResource,
    SchemaResource,
    LogsResource,
    DevResource,
    FilesResource,
    HealthResource,
)

# Exception hierarchy
from .exceptions import (
    AlcedoError,
    AuthenticationError,
    ConnectionError,  # backward-compat alias for ConnectionFailedError
    ConnectionFailedError,
    KeyNotFoundError,
    KVStoreError,
    NotFoundError,
    ServerError,
    ValidationError,
)

# Backward-compatible wrapper (deprecated)
from .client import AlcedoKV

__all__ = [
    # Primary
    "AlcedoClient",
    # Resources
    "KVResource",
    "DBResource",
    "SettingsResource",
    "MigrationsResource",
    "SchemaResource",
    "LogsResource",
    "DevResource",
    "FilesResource",
    "HealthResource",
    # Exceptions
    "AlcedoError",
    "AuthenticationError",
    "ConnectionFailedError",
    "NotFoundError",
    "ServerError",
    "ValidationError",
    # Backward compat (deprecated)
    "AlcedoKV",
    "KeyNotFoundError",
    "KVStoreError",
]
