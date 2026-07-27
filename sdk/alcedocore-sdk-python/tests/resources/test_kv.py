import httpx
import pytest
import respx

from alcedo_sdk.resources.kv import KVResource
from alcedo_sdk.exceptions import NotFoundError, ServerError

BASE_URL = "http://localhost:8080"


@pytest.fixture
def kv():
    return KVResource(httpx.AsyncClient(base_url=BASE_URL), "test-plugin")


# ── GET ──

@pytest.mark.asyncio
async def test_get(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/my-key").respond(json={"data": "hello"}, status_code=200)
        result = await kv.get("my-key")
        assert result == "hello"


@pytest.mark.asyncio
async def test_get_missing(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/my-key").respond(json={}, status_code=200)
        result = await kv.get("my-key")
        assert result is None


@pytest.mark.asyncio
async def test_get_not_found(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/my-key").respond(status_code=404)
        with pytest.raises(NotFoundError):
            await kv.get("my-key")


# ── SET ──

@pytest.mark.asyncio
async def test_set(kv):
    with respx.mock as mock:
        mock.put(f"{BASE_URL}/api/kv/my-key", json={"value": "hello"}).respond(
            json={"data": "hello"}, status_code=200
        )
        result = await kv.set("my-key", "hello")
        assert result == "hello"


@pytest.mark.asyncio
async def test_set_with_ttl(kv):
    with respx.mock as mock:
        mock.put(
            f"{BASE_URL}/api/kv/my-key", json={"value": "hello"}, params={"ttl": "60"}
        ).respond(json={"data": "hello"}, status_code=200)
        result = await kv.set("my-key", "hello", ttl=60)
        assert result == "hello"


# ── DELETE ──

@pytest.mark.asyncio
async def test_delete(kv):
    with respx.mock as mock:
        mock.delete(f"{BASE_URL}/api/kv/my-key").respond(status_code=204)
        result = await kv.delete("my-key")
        assert result is True


@pytest.mark.asyncio
async def test_delete_not_found(kv):
    with respx.mock as mock:
        mock.delete(f"{BASE_URL}/api/kv/my-key").respond(status_code=404)
        result = await kv.delete("my-key")
        assert result is False


# ── EXISTS ──

@pytest.mark.asyncio
async def test_exists_true(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/my-key/exists").respond(json={"exists": True}, status_code=200)
        result = await kv.exists("my-key")
        assert result is True


@pytest.mark.asyncio
async def test_exists_false(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/my-key/exists").respond(json={"exists": False}, status_code=200)
        result = await kv.exists("my-key")
        assert result is False


# ── TTL ──

@pytest.mark.asyncio
async def test_ttl(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/my-key/ttl").respond(json={"ttl": 42}, status_code=200)
        result = await kv.ttl("my-key")
        assert result == 42


@pytest.mark.asyncio
async def test_ttl_none(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/my-key/ttl").respond(json={"ttl": None}, status_code=200)
        result = await kv.ttl("my-key")
        assert result is None


# ── LIST KEYS ──

@pytest.mark.asyncio
async def test_list_keys(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/").respond(json={"keys": ["a", "b", "c"]}, status_code=200)
        result = await kv.list_keys()
        assert result == ["a", "b", "c"]


@pytest.mark.asyncio
async def test_list_keys_with_prefix(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/", params={"prefix": "test"}).respond(
            json={"keys": ["test-a"]}, status_code=200
        )
        result = await kv.list_keys(prefix="test")
        assert result == ["test-a"]


# ── BATCH GET ──

@pytest.mark.asyncio
async def test_batch_get(kv):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/kv/batch/get", json={"keys": ["a", "b"]}).respond(
            json={"values": {"a": "1", "b": "2"}}, status_code=200
        )
        result = await kv.batch_get(["a", "b"])
        assert result == {"a": "1", "b": "2"}


# ── BATCH SET ──

@pytest.mark.asyncio
async def test_batch_set(kv):
    with respx.mock as mock:
        route = mock.post(
            f"{BASE_URL}/api/kv/batch/set",
            json=[{"key": "a", "value": "1"}, {"key": "b", "value": "2"}],
        ).respond(status_code=200)
        await kv.batch_set([{"key": "a", "value": "1"}, {"key": "b", "value": "2"}])
        assert route.called


# ── BATCH DELETE ──

@pytest.mark.asyncio
async def test_batch_delete(kv):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/kv/batch/delete", json={"keys": ["a", "b"]}).respond(
            json={"deleted": 2}, status_code=200
        )
        result = await kv.batch_delete(["a", "b"])
        assert result == 2


# ── ERROR MAPPING ──

@pytest.mark.asyncio
async def test_server_error(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/kv/my-key").respond(status_code=500)
        with pytest.raises(ServerError):
            await kv.get("my-key")
