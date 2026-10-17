import pytest
import respx

from alcedo_sdk.resources.kv import KVResource
from alcedo_sdk.exceptions import AlcedoApiError, NotFoundError, ServerError
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"


@pytest.fixture
def kv(http_client):
    return KVResource(http_client, "test-plugin")


# ── GET ──

@pytest.mark.asyncio
async def test_get(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv/my-key").respond(
            json=jsend({"key": "my-key", "value": "hello"})
        )
        result = await kv.get("my-key")
        assert result == {"key": "my-key", "value": "hello"}


@pytest.mark.asyncio
async def test_get_not_found(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv/my-key").respond(status_code=404)
        with pytest.raises(NotFoundError):
            await kv.get("my-key")


# ── SET ──

@pytest.mark.asyncio
async def test_set(kv):
    with respx.mock as mock:
        mock.put(f"{BASE_URL}/api/app/kv/my-key", json={"value": "hello"}).respond(
            json=jsend({"key": "my-key", "value": "hello"})
        )
        result = await kv.set("my-key", "hello")
        assert result == {"key": "my-key", "value": "hello"}


@pytest.mark.asyncio
async def test_set_with_ttl(kv):
    with respx.mock as mock:
        mock.put(
            f"{BASE_URL}/api/app/kv/my-key", json={"value": "hello"}, params={"ttl": "60"}
        ).respond(json=jsend({"key": "my-key", "value": "hello"}))
        result = await kv.set("my-key", "hello", ttl=60)
        assert result == {"key": "my-key", "value": "hello"}


@pytest.mark.asyncio
async def test_set_object_json_encoded(kv):
    with respx.mock as mock:
        route = mock.put(f"{BASE_URL}/api/app/kv/my-key").respond(json=jsend())
        await kv.set("my-key", {"a": 1})
        import json as jsonlib

        body = jsonlib.loads(route.calls.last.request.content)
        assert body == {"value": '{"a": 1}'}


# ── DELETE ──

@pytest.mark.asyncio
async def test_delete(kv):
    with respx.mock as mock:
        mock.delete(f"{BASE_URL}/api/app/kv/my-key").respond(
            json=jsend({"key": "my-key", "deleted": True})
        )
        result = await kv.delete("my-key")
        assert result == {"key": "my-key", "deleted": True}


# ── EXISTS ──

@pytest.mark.asyncio
async def test_exists(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv/my-key/exists").respond(
            json=jsend({"key": "my-key", "exists": True})
        )
        result = await kv.exists("my-key")
        assert result == {"key": "my-key", "exists": True}


# ── TTL ──

@pytest.mark.asyncio
async def test_ttl(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv/my-key/ttl").respond(
            json=jsend({"key": "my-key", "ttl": 42})
        )
        result = await kv.ttl("my-key")
        assert result == {"key": "my-key", "ttl": 42}


# ── LIST KEYS ──

@pytest.mark.asyncio
async def test_list(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv").respond(
            json=jsend({"keys": ["a", "b", "c"]})
        )
        result = await kv.list()
        assert result == {"keys": ["a", "b", "c"]}


@pytest.mark.asyncio
async def test_list_with_prefix(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv", params={"prefix": "test"}).respond(
            json=jsend({"keys": ["test-a"]})
        )
        result = await kv.list(prefix="test")
        assert result == {"keys": ["test-a"]}


@pytest.mark.asyncio
async def test_list_keys_alias(kv):
    """The old Python name list_keys aliases the node name list."""
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv").respond(json=jsend({"keys": ["a"]}))
        result = await kv.list_keys()
        assert result == {"keys": ["a"]}


# ── BATCH ──

@pytest.mark.asyncio
async def test_batch_get(kv):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/app/kv/batch/get", json={"keys": ["a", "b"]}).respond(
            json=jsend({"values": {"a": "1", "b": "2"}})
        )
        result = await kv.batch_get(["a", "b"])
        assert result == {"values": {"a": "1", "b": "2"}}


@pytest.mark.asyncio
async def test_batch_set(kv):
    with respx.mock as mock:
        route = mock.post(
            f"{BASE_URL}/api/app/kv/batch/set",
            json=[{"key": "a", "value": "1"}],
        ).respond(json=jsend(None))
        await kv.batch_set([{"key": "a", "value": "1"}])
        assert route.called


@pytest.mark.asyncio
async def test_batch_delete(kv):
    with respx.mock as mock:
        mock.post(f"{BASE_URL}/api/app/kv/batch/delete", json={"keys": ["a", "b"]}).respond(
            json=jsend({"deleted": 2})
        )
        result = await kv.batch_delete(["a", "b"])
        assert result == {"deleted": 2}


# ── QUERY ──

@pytest.mark.asyncio
async def test_query(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv/query", params={"pattern": "sess:*"}).respond(
            json=jsend({"status": "ok"})
        )
        result = await kv.query(pattern="sess:*")
        assert result == {"status": "ok"}


# ── APP CONTEXT HEADERS ──

@pytest.mark.asyncio
async def test_app_headers_per_call(kv):
    with respx.mock as mock:
        route = mock.get(f"{BASE_URL}/api/app/kv/my-key").respond(json=jsend({"key": "my-key", "value": "v"}))
        await kv.get("my-key", app="blog", version="production")
        request = route.calls.last.request
        assert request.headers["X-App"] == "blog"
        assert request.headers["X-Version"] == "production"


# ── ERROR MAPPING ──

@pytest.mark.asyncio
async def test_server_error(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv/my-key").respond(status_code=500)
        with pytest.raises(ServerError):
            await kv.get("my-key")


@pytest.mark.asyncio
async def test_jsend_fail_envelope(kv):
    with respx.mock as mock:
        mock.get(f"{BASE_URL}/api/app/kv/my-key").respond(
            json={"status": "fail", "message": "no app context"}, status_code=400
        )
        with pytest.raises(AlcedoApiError) as exc_info:
            await kv.get("my-key")
        assert exc_info.value.status == "fail"
