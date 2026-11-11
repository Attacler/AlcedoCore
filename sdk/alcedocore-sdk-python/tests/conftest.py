import httpx
import pytest

BASE_URL = "http://localhost:8080"


@pytest.fixture
def base_url() -> str:
    return BASE_URL


@pytest.fixture
def plugin_slug() -> str:
    return "test-plugin"


@pytest.fixture
def http_client() -> httpx.AsyncClient:
    return httpx.AsyncClient(base_url=BASE_URL)
