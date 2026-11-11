import pytest
import respx

from alcedo_sdk.resources.migrations import MigrationsResource
from alcedo_sdk.exceptions import AlcedoApiError, NotFoundError
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def migrations(http_client):
    return MigrationsResource(http_client, "test-plugin")


@pytest.mark.asyncio
async def test_list_migrations(migrations):
    expected = [
        {"version": "001", "name": "init", "status": "applied"},
        {"version": "002", "name": "add_field", "status": "pending"},
    ]
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/platform/plugins/test-plugin/migrations").respond(
            json=jsend(expected)
        )
        result = await migrations.list()
        assert result == expected


@pytest.mark.asyncio
async def test_list_migrations_explicit_slug(migrations):
    with respx.mock as mock:
        route = mock.get(f"{BASE_URL}/api/platform/plugins/other/migrations").respond(
            json=jsend([])
        )
        await migrations.list(slug="other")
        assert route.called


@pytest.mark.asyncio
async def test_run_migrations(migrations):
    """run() posts to the migrations endpoint and returns None (node parity)."""
    with respx.mock as mock:
        route = mock.post(f"{BASE_URL}/api/platform/plugins/test-plugin/migrations").respond(
            json=jsend({"applied": ["002_add_field"], "errors": []})
        )
        result = await migrations.run()
        assert result is None
        assert route.called


@pytest.mark.asyncio
async def test_rollback(migrations):
    expected = {"rolled_back": "002_add_field", "status": "success"}
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/platform/plugins/test-plugin/rollback/002_add_field").respond(
            json=jsend(expected)
        )
        result = await migrations.rollback("002_add_field")
        assert result == expected


@pytest.mark.asyncio
async def test_migration_not_found(migrations):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/platform/plugins/test-plugin/rollback/nonexistent").respond(status_code=404)
        with pytest.raises(NotFoundError):
            await migrations.rollback("nonexistent")
