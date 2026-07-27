import httpx
import pytest
import respx

from alcedo_sdk.resources.migrations import MigrationsResource
from alcedo_sdk.exceptions import NotFoundError

BASE_URL = "http://localhost:8080"


@pytest.fixture
def migrations():
    return MigrationsResource(httpx.AsyncClient(base_url=BASE_URL), "test-plugin")


@pytest.mark.asyncio
async def test_list_migrations(migrations):
    expected = [
        {"version": "001", "name": "init", "status": "applied"},
        {"version": "002", "name": "add_field", "status": "pending"},
    ]
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/plugins/test-plugin/migrations").respond(json=expected, status_code=200)
        result = await migrations.list()
        assert result == expected


@pytest.mark.asyncio
async def test_run_migrations(migrations):
    expected = {"applied": ["002_add_field"], "errors": []}
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/plugins/test-plugin/migrations").respond(json=expected, status_code=200)
        result = await migrations.run()
        assert result == expected


@pytest.mark.asyncio
async def test_rollback(migrations):
    expected = {"rolled_back": "002_add_field", "status": "success"}
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/plugins/test-plugin/rollback/002_add_field").respond(
            json=expected, status_code=200
        )
        result = await migrations.rollback("002_add_field")
        assert result == expected


@pytest.mark.asyncio
async def test_migration_not_found(migrations):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/plugins/test-plugin/rollback/nonexistent").respond(status_code=404)
        with pytest.raises(NotFoundError):
            await migrations.rollback("nonexistent")
