import pytest
import respx

from alcedo_sdk.resources.plugins import PluginsResource
from alcedo_sdk.exceptions import NotFoundError
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
PLUGINS = f"{BASE_URL}/api/platform/plugins"


@pytest.fixture
def plugins(http_client):
    return PluginsResource(http_client, "test-plugin")


@pytest.mark.asyncio
async def test_list(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}").respond(json=jsend([{"slug": "hello-world"}]))
        result = await plugins.list()
        assert result == [{"slug": "hello-world"}]


@pytest.mark.asyncio
async def test_get(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}/hello-world").respond(json=jsend({"slug": "hello-world"}))
        result = await plugins.get("hello-world")
        assert result == {"slug": "hello-world"}


@pytest.mark.asyncio
async def test_update(plugins):
    with respx.mock as mock:
        mock.put(f"{PLUGINS}/hello-world", json={"name": "Hello"}).respond(
            json=jsend({"slug": "hello-world", "name": "Hello"})
        )
        result = await plugins.update("hello-world", {"name": "Hello"})
        assert result["name"] == "Hello"


@pytest.mark.asyncio
async def test_delete(plugins):
    with respx.mock as mock:
        mock.delete(f"{PLUGINS}/hello-world").respond(json=jsend({"success": True}))
        result = await plugins.delete("hello-world")
        assert result == {"success": True}


@pytest.mark.asyncio
async def test_deploy(plugins):
    body = {
        "slug": "hello-world",
        "version": "1.0.0",
        "image": "localhost:5000/hello-world:1.0.0",
        "registry_id": 1,
        "env": {},
    }
    with respx.mock as mock:
        mock.post(f"{PLUGINS}/deploy", json=body).respond(json=jsend({"slug": "hello-world"}))
        result = await plugins.deploy(body)
        assert result == {"slug": "hello-world"}


@pytest.mark.asyncio
async def test_preview(plugins):
    with respx.mock as mock:
        mock.post(
            f"{PLUGINS}/preview",
            json={"image": "localhost:5000/hello-world:1.0.0", "registry_id": 2},
        ).respond(json=jsend({"manifest": {}}))
        result = await plugins.preview("localhost:5000/hello-world:1.0.0", registry_id=2)
        assert result == {"manifest": {}}


# ── install-scoped (slug + app_version_id) ──

@pytest.mark.asyncio
async def test_uninstall(plugins):
    with respx.mock as mock:
        mock.delete(f"{PLUGINS}/hello-world/installs/7").respond(json=jsend({"success": True}))
        result = await plugins.uninstall("hello-world", 7)
        assert result == {"success": True}


@pytest.mark.asyncio
async def test_enable_disable(plugins):
    with respx.mock as mock:
        enable = mock.post(f"{PLUGINS}/hello-world/installs/7/enable").respond(json=jsend({"enabled": True}))
        disable = mock.post(f"{PLUGINS}/hello-world/installs/7/disable").respond(json=jsend({"enabled": False}))
        await plugins.enable("hello-world", 7)
        await plugins.disable("hello-world", 7)
        assert enable.called and disable.called


@pytest.mark.asyncio
async def test_get_settings(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}/hello-world/installs/7/settings").respond(
            json=jsend({"settings": {"greeting": "Hello"}, "schema": {}})
        )
        result = await plugins.get_settings("hello-world", 7)
        assert result == {"settings": {"greeting": "Hello"}, "schema": {}}


@pytest.mark.asyncio
async def test_update_settings(plugins):
    with respx.mock as mock:
        mock.patch(f"{PLUGINS}/hello-world/installs/7/settings", json={"greeting": "Hi"}).respond(
            json=jsend({"settings": {"greeting": "Hi"}})
        )
        result = await plugins.update_settings("hello-world", 7, {"greeting": "Hi"})
        assert result == {"settings": {"greeting": "Hi"}}


@pytest.mark.asyncio
async def test_get_scopes(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}/hello-world/installs/7/scopes").respond(
            json=jsend({"requested": ["kv.get"], "granted": ["kv.get"]})
        )
        result = await plugins.get_scopes("hello-world", 7)
        assert result == {"requested": ["kv.get"], "granted": ["kv.get"]}


@pytest.mark.asyncio
async def test_update_scopes(plugins):
    with respx.mock as mock:
        mock.post(f"{PLUGINS}/hello-world/installs/7/scopes", json={"scopes": ["kv.get"]}).respond(
            json=jsend({"granted": ["kv.get"]})
        )
        result = await plugins.update_scopes("hello-world", 7, ["kv.get"])
        assert result == {"granted": ["kv.get"]}


# ── runtime (keyed by install id) ──

@pytest.mark.asyncio
async def test_runtime_info(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}/installs/42/runtime").respond(json=jsend({"status": "running"}))
        result = await plugins.runtime_info(42)
        assert result == {"status": "running"}


@pytest.mark.asyncio
async def test_instances(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}/installs/42/instances").respond(json=jsend([{"task_id": "abc"}]))
        result = await plugins.instances(42)
        assert result == [{"task_id": "abc"}]


@pytest.mark.asyncio
async def test_instance_detail_stats_logs(plugins):
    with respx.mock as mock:
        detail = mock.get(f"{PLUGINS}/installs/42/instances/abc").respond(json=jsend({"task_id": "abc"}))
        stats = mock.get(f"{PLUGINS}/installs/42/instances/abc/stats").respond(json=jsend({"cpu": 0.1}))
        logs = mock.get(f"{PLUGINS}/installs/42/instances/abc/logs").respond(json=jsend(["line 1"]))
        await plugins.instance_detail(42, "abc")
        await plugins.instance_stats(42, "abc")
        await plugins.instance_logs(42, "abc")
        assert detail.called and stats.called and logs.called


# ── slug-scoped subresources ──

@pytest.mark.asyncio
async def test_versions_docs_pages_assets(plugins):
    with respx.mock as mock:
        versions = mock.get(f"{PLUGINS}/hello-world/versions").respond(json=jsend([{"version": "1.0.0"}]))
        docs = mock.get(f"{PLUGINS}/hello-world/docs").respond(json=jsend([{"path": "README.md"}]))
        pages = mock.get(f"{PLUGINS}/hello-world/pages").respond(json=jsend({"pages": []}))
        assets = mock.get(f"{PLUGINS}/hello-world/pages/assets").respond(json=jsend({"assets": []}))
        await plugins.versions("hello-world")
        await plugins.docs("hello-world")
        await plugins.pages("hello-world")
        await plugins.assets("hello-world")
        assert versions.called and docs.called and pages.called and assets.called


@pytest.mark.asyncio
async def test_doc_content_returns_text(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}/hello-world/docs/README.md").respond(text="# Hello", status_code=200)
        result = await plugins.doc_content("hello-world", "docs/README.md")
        assert result == "# Hello"


@pytest.mark.asyncio
async def test_request_logs(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}/hello-world/logs", params={"limit": "10"}).respond(json=jsend([]))
        result = await plugins.request_logs("hello-world", params={"limit": "10"})
        assert result == []


@pytest.mark.asyncio
async def test_scale_and_restart(plugins):
    with respx.mock as mock:
        scale = mock.post(f"{PLUGINS}/hello-world/scale", json={"instances": 2}).respond(
            json=jsend({"scaled": True})
        )
        restart = mock.post(
            f"{PLUGINS}/hello-world/restart", json={"deployment_id": "abc123"}
        ).respond(json=jsend({"restarted": True}))
        await plugins.scale("hello-world", {"instances": 2})
        await plugins.restart("hello-world", "abc123")
        assert scale.called and restart.called


@pytest.mark.asyncio
async def test_not_found(plugins):
    with respx.mock as mock:
        mock.get(f"{PLUGINS}/missing").respond(status_code=404)
        with pytest.raises(NotFoundError):
            await plugins.get("missing")
