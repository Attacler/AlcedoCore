import httpx
import pytest


@pytest.fixture
def base_url() -> str:
    return "http://localhost:8080"


@pytest.fixture
def plugin_slug() -> str:
    return "test-plugin"
