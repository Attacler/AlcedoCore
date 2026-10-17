import pytest
import respx

from alcedo_sdk.resources.sessions import SessionsResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
SESSIONS = f"{BASE_URL}/api/platform/sessions"


@pytest.fixture
def sessions(http_client):
    return SessionsResource(http_client)


@pytest.mark.asyncio
async def test_list(sessions):
    with respx.mock as mock:
        mock.get(f"{SESSIONS}").respond(json=jsend([{"id": "s1", "current": True}]))
        result = await sessions.list()
        assert result == [{"id": "s1", "current": True}]


@pytest.mark.asyncio
async def test_revoke(sessions):
    with respx.mock as mock:
        mock.delete(f"{SESSIONS}/s1").respond(json=jsend(True))
        result = await sessions.revoke("s1")
        assert result is True


@pytest.mark.asyncio
async def test_revoke_all(sessions):
    with respx.mock as mock:
        mock.delete(f"{SESSIONS}").respond(json=jsend(True))
        result = await sessions.revoke_all()
        assert result is True
