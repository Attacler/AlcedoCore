import pytest
import respx

from alcedo_sdk.exceptions import AlcedoApiError
from alcedo_sdk.resources.versions import VersionsResource
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
VERSIONS = f"{BASE_URL}/api/platform/versions"


@pytest.fixture
def versions(http_client):
    return VersionsResource(http_client)


@pytest.mark.asyncio
async def test_list(versions):
    with respx.mock as mock:
        mock.get(f"{VERSIONS}").respond(json=jsend([{"id": 1, "version_name": "production"}]))
        result = await versions.list()
        assert result == [{"id": 1, "version_name": "production"}]


@pytest.mark.asyncio
async def test_create(versions):
    with respx.mock as mock:
        mock.post(f"{VERSIONS}", json={"version_name": "staging"}).respond(
            json=jsend({"id": 2, "version_name": "staging"})
        )
        result = await versions.create("staging")
        assert result == {"id": 2, "version_name": "staging"}


@pytest.mark.asyncio
async def test_remove(versions):
    with respx.mock as mock:
        mock.delete(f"{VERSIONS}/2").respond(json=jsend({"success": True}))
        result = await versions.remove(2)
        assert result == {"success": True}


@pytest.mark.asyncio
async def test_deploy_version(versions):
    result = {"deployed": True, "apps": {"crm": {"tables_written": 3, "rows_copied": 12}}}
    with respx.mock as mock:
        mock.post(
            f"{VERSIONS}/3/deploy",
            json={"dry_run": False, "on_conflict": "fail", "edit_ids": None},
        ).respond(json=jsend(result))
        out = await versions.deploy_version(3)
        assert out == result


@pytest.mark.asyncio
async def test_deploy_version_dry_run(versions):
    result = {"deployed": False, "dry_run": True, "edits": [{"id": 11}], "conflicts": []}
    with respx.mock as mock:
        mock.post(
            f"{VERSIONS}/3/deploy",
            json={"dry_run": True, "on_conflict": "fail", "edit_ids": None},
        ).respond(json=jsend(result))
        out = await versions.deploy_version(3, dry_run=True)
        assert out == result


@pytest.mark.asyncio
async def test_deploy_version_edit_ids(versions):
    with respx.mock as mock:
        mock.post(
            f"{VERSIONS}/3/deploy",
            json={"dry_run": False, "on_conflict": "skip", "edit_ids": [11, 12]},
        ).respond(json=jsend({"deployed": True}))
        out = await versions.deploy_version(3, on_conflict="skip", edit_ids=[11, 12])
        assert out == {"deployed": True}


@pytest.mark.asyncio
async def test_deploy_version_conflict_fail_raises(versions):
    conflicts = [
        {
            "type": "changed_on_both_sides",
            "app": "crm",
            "collection": "contacts",
            "item_id": "1",
            "detail": "changed on both sides",
        }
    ]
    with respx.mock as mock:
        mock.post(f"{VERSIONS}/3/deploy").respond(
            json={
                "status": "fail",
                "message": "deploy conflicts",
                "data": {"deployed": False, "conflicts": conflicts},
            },
            status_code=409,
        )
        with pytest.raises(AlcedoApiError) as exc_info:
            await versions.deploy_version(3, on_conflict="fail")
        assert exc_info.value.status == "fail"
        assert exc_info.value.http_status == 409
        # the JSend data payload is carried on the error (data.conflicts reachable)
        assert exc_info.value.data == {"deployed": False, "conflicts": conflicts}
        assert exc_info.value.data["conflicts"] == conflicts


@pytest.mark.asyncio
async def test_revert_version(versions):
    result = {
        "version_id": 3,
        "version_name": "staging",
        "reverted": [{"id": 11, "action": "collection_created"}],
        "skipped": [],
        "warning": "No data backup is created before deploy/revert.",
    }
    with respx.mock as mock:
        mock.post(f"{VERSIONS}/3/revert").respond(json=jsend(result))
        out = await versions.revert_version(3)
        assert out == result
