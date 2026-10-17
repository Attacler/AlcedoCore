"""Alcedo Python SDK — unified typed client for Alcedo Plugin Core."""

# Primary API
from .client import AlcedoClient

# Resource modules
from .resources import (
    ActivityLogsResource,
    AppSettingsResource,
    AppsResource,
    AuthResource,
    CollectionsResource,
    DBResource,
    DeveloperApiKeysResource,
    FilesResource,
    FoldersResource,
    HealthResource,
    ItemsResource,
    KVResource,
    LookupResource,
    LogsResource,
    MenusResource,
    MigrationsResource,
    PluginsResource,
    PoliciesResource,
    RegistriesResource,
    RolesResource,
    SchemaResource,
    SessionsResource,
    UsageResource,
    UsersResource,
    VersionsResource,
)

# Exception hierarchy
from .exceptions import (
    AlcedoApiError,
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
    "ActivityLogsResource",
    "AppSettingsResource",
    "AppsResource",
    "AuthResource",
    "CollectionsResource",
    "DBResource",
    "DeveloperApiKeysResource",
    "FilesResource",
    "FoldersResource",
    "HealthResource",
    "ItemsResource",
    "KVResource",
    "LookupResource",
    "LogsResource",
    "MenusResource",
    "MigrationsResource",
    "PluginsResource",
    "PoliciesResource",
    "RegistriesResource",
    "RolesResource",
    "SchemaResource",
    "SessionsResource",
    "UsageResource",
    "UsersResource",
    "VersionsResource",
    # Exceptions
    "AlcedoApiError",
    "AlcedoError",
    "ConnectionFailedError",
    "NotFoundError",
    "ServerError",
    "ValidationError",
    # Backward compat (deprecated)
    "AlcedoKV",
    "KeyNotFoundError",
    "KVStoreError",
]
