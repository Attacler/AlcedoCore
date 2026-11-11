import pytest
import respx

from alcedo_sdk.resources.db import DBResource
from alcedo_sdk.exceptions import AlcedoApiError, ServerError, ValidationError
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def db(http_client):
    return DBResource(http_client, "test-plugin")


@pytest.fixture
def plugin_base_path(monkeypatch):
    """The core injects PLUGIN_BASE_PATH=/p/{install_id} into every plugin."""
    monkeypatch.setenv("PLUGIN_BASE_PATH", "/p/7")
    return "/p/7"


@pytest.mark.asyncio
async def test_query_uses_plugin_base_path_env(db, plugin_base_path):
    expected = {
        "columns": ["id", "name"],
        "rows": [[1, "hello"]],
        "row_count": 1,
        "truncated": False,
        "execution_time_ms": 5.0,
    }
    with respx.mock as mock:
        mock.post(
            f"{BASE_URL}/p/7/db/query",
            json={"query": "SELECT * FROM items", "params": [], "timeout_secs": 30, "max_rows": 100},
        ).respond(json=jsend(expected))
        result = await db.query("SELECT * FROM items")
        assert result == expected


@pytest.mark.asyncio
async def test_query_with_params(db, plugin_base_path):
    with respx.mock as mock:
        route = mock.post(
            f"{BASE_URL}/p/7/db/query",
            json={"query": "SELECT * FROM items WHERE id = $1", "params": [1], "timeout_secs": 10, "max_rows": 50},
        ).respond(json=jsend({"row_count": 1}))
        await db.query("SELECT * FROM items WHERE id = $1", params=[1], timeout_secs=10, max_rows=50)
        assert route.called


@pytest.mark.asyncio
async def test_query_explicit_install_id(db, monkeypatch):
    monkeypatch.delenv("PLUGIN_BASE_PATH", raising=False)
    with respx.mock as mock:
        route = mock.post(f"{BASE_URL}/p/42/db/query").respond(json=jsend({"row_count": 0}))
        await db.query("SELECT 1", install_id=42)
        assert route.called


@pytest.mark.asyncio
async def test_query_explicit_base_path_overrides_env(db, plugin_base_path):
    with respx.mock as mock:
        route = mock.post(f"{BASE_URL}/p/99/db/query").respond(json=jsend({"row_count": 0}))
        await db.query("SELECT 1", base_path="/p/99/")
        assert route.called


@pytest.mark.asyncio
async def test_query_without_base_path_raises(db, monkeypatch):
    monkeypatch.delenv("PLUGIN_BASE_PATH", raising=False)
    with pytest.raises(ValueError, match="PLUGIN_BASE_PATH"):
        await db.query("SELECT 1")


@pytest.mark.asyncio
async def test_query_validation_error(db, plugin_base_path):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/p/7/db/query").respond(
            json={"status": "fail", "message": "syntax error"}, status_code=400
        )
        with pytest.raises(AlcedoApiError, match="syntax error"):
            await db.query("INVALID SQL")


@pytest.mark.asyncio
async def test_query_server_error(db, plugin_base_path):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/p/7/db/query").respond(status_code=500)
        with pytest.raises(ServerError):
            await db.query("SELECT 1")
