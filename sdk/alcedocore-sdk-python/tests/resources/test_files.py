import pytest
import respx

from alcedo_sdk.resources.files import FilesResource, resolve_download_url
from tests.helpers import jsend

BASE_URL = "http://localhost:8080"
FILES = f"{BASE_URL}/api/app/files"


@pytest.fixture
def files(http_client):
    return FilesResource(http_client, "test-plugin", base_url=BASE_URL)


@pytest.mark.asyncio
async def test_upload_from_path(files, tmp_path):
    upload = tmp_path / "hello.txt"
    upload.write_bytes(b"hello world")
    with respx.mock as mock:
        route = mock.post(f"{FILES}/upload").respond(json=jsend({"id": "f1", "filename": "hello.txt"}))
        result = await files.upload(str(upload))
        assert result["id"] == "f1"
        request = route.calls.last.request
        assert "multipart/form-data" in request.headers["content-type"]
        assert b"hello world" in request.content
        assert b'filename="hello.txt"' in request.content


@pytest.mark.asyncio
async def test_upload_with_options(files, tmp_path):
    upload = tmp_path / "hello.txt"
    upload.write_bytes(b"hello")
    with respx.mock as mock:
        route = mock.post(f"{FILES}/upload").respond(json=jsend({"id": "f1"}))
        await files.upload(
            str(upload),
            collection_name="posts",
            item_id="i1",
            field_name="attachment",
            folder_id="fold1",
            overwrite=True,
        )
        content = route.calls.last.request.content
        assert b"collection_name" in content
        assert b"posts" in content
        assert b"item_id" in content
        assert b"field_name" in content
        assert b"folder_id" in content
        assert b"overwrite" in content


@pytest.mark.asyncio
async def test_download(files):
    with respx.mock as mock:
        mock.get(f"{FILES}/f1/download").respond(content=b"file-bytes", status_code=200)
        result = await files.download("f1")
        assert result == b"file-bytes"


@pytest.mark.asyncio
async def test_get_list_update_delete(files):
    media = {"id": "f1", "filename": "hello.txt", "mime_type": "text/plain"}
    with respx.mock as mock:
        got = mock.get(f"{FILES}/f1").respond(json=jsend(media))
        listing = mock.get(f"{FILES}", params={"limit": 10, "search": "hello"}).respond(
            json=jsend({"data": [media], "total": 1})
        )
        updated = mock.patch(f"{FILES}/f1", json={"alt_text": "greeting"}).respond(
            json=jsend(media)
        )
        deleted = mock.delete(f"{FILES}/f1").respond(json=jsend({"success": True}))
        batch_deleted = mock.post(f"{FILES}/batch/delete", json={"ids": ["f1"]}).respond(
            json=jsend({"deleted": 1})
        )
        await files.get("f1")
        await files.list(limit=10, search="hello")
        await files.update("f1", {"alt_text": "greeting"})
        await files.delete("f1")
        await files.batch_delete(["f1"])
        assert got.called and listing.called and updated.called and deleted.called
        assert batch_deleted.called


@pytest.mark.asyncio
async def test_folders(files):
    with respx.mock as mock:
        created = mock.post(f"{FILES}/folders", json={"name": "images"}).respond(
            json=jsend({"id": "fold1"})
        )
        listing = mock.get(f"{FILES}/folders", params={"parent_id": "fold1"}).respond(
            json=jsend({"data": []})
        )
        got = mock.get(f"{FILES}/folders/fold1").respond(json=jsend({"id": "fold1"}))
        updated = mock.patch(f"{FILES}/folders/fold1", json={"name": "img"}).respond(
            json=jsend({"id": "fold1"})
        )
        deleted = mock.delete(f"{FILES}/folders/fold1", params={"recursive": "true"}).respond(
            json=jsend({"success": True})
        )
        await files.folders.create("images")
        await files.folders.list(parent_id="fold1")
        await files.folders.get("fold1")
        await files.folders.update("fold1", {"name": "img"})
        await files.folders.delete("fold1", recursive=True)
        assert created.called and listing.called and got.called and updated.called and deleted.called


def test_download_url_from_metadata():
    url = resolve_download_url(BASE_URL, "blog", "production", {"download_url": "/api/app/files/f1/download"})
    assert url == "/api/app/files/f1/download"


def test_download_url_from_id():
    url = resolve_download_url(BASE_URL, "blog", "production", "f1")
    assert url == f"{BASE_URL}/api/app/files/f1/download?ac_app=blog&ac_version=production"


def test_download_url_empty_for_missing_id():
    assert resolve_download_url(BASE_URL, None, None, None) == ""


def test_download_url_per_call_override():
    url = resolve_download_url(BASE_URL, "blog", "production", "f1", app="shop", version="staging")
    assert url == f"{BASE_URL}/api/app/files/f1/download?ac_app=shop&ac_version=staging"


def test_client_download_url_method(files):
    assert files.download_url("f1") == (
        f"{BASE_URL}/api/app/files/f1/download"
    )
    assert files.download_url("f1", app="blog") == (
        f"{BASE_URL}/api/app/files/f1/download?ac_app=blog"
    )
