import warnings

import pytest

from alcedo_sdk.exceptions import (
    AlcedoError,
    AuthenticationError,
    ConnectionError,  # backward-compat alias
    ConnectionFailedError,
    KeyNotFoundError,
    NotFoundError,
    ServerError,
    ValidationError,
    KVStoreError,
)


class TestAlcedoError:
    """Exception hierarchy tests."""

    def test_hierarchy(self):
        """Every AlcedoError subtype is instance of AlcedoError."""
        assert isinstance(ConnectionFailedError("msg"), AlcedoError)
        assert isinstance(ConnectionError("msg"), AlcedoError)  # backward-compat alias
        assert isinstance(NotFoundError("msg"), AlcedoError)
        assert isinstance(ValidationError("msg"), AlcedoError)
        assert isinstance(AuthenticationError("msg"), AlcedoError)
        assert isinstance(ServerError("msg"), AlcedoError)

    def test_str_with_status_code(self):
        """str() includes status code when set."""
        err = NotFoundError("nope", status_code=404)
        assert "404" in str(err)

    def test_str_without_status_code(self):
        """str() returns only message when no status code."""
        err = AlcedoError("nope")
        assert str(err) == "nope"

    def test_connection_error_original(self):
        """ConnectionFailedError supports .original attribute."""
        err = ConnectionFailedError("network error")
        err.original = RuntimeError("underlying error")
        assert isinstance(err.original, RuntimeError)

    def test_not_found_error_key(self):
        """NotFoundError stores key."""
        err = NotFoundError("nope", key="my-key")
        assert err.key == "my-key"

    def test_validation_error_details(self):
        """ValidationError stores details dict."""
        err = ValidationError("bad", details={"field": "name"})
        assert err.details == {"field": "name"}

    def test_kv_store_error_alias(self):
        """KVStoreError is AlcedoError at module level."""
        assert KVStoreError is AlcedoError

    def test_key_not_found_deprecation(self):
        """KeyNotFoundError fires DeprecationWarning."""
        with pytest.warns(DeprecationWarning) as record:
            KeyNotFoundError("my-key")
        assert len(record) == 1
        assert "KeyNotFoundError is deprecated" in str(record[0].message)

    def test_key_not_found_message(self):
        """KeyNotFoundError message includes the key."""
        with pytest.warns(DeprecationWarning):
            exc = KeyNotFoundError("my-key")
        assert "my-key" in str(exc)
