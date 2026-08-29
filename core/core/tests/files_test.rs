//! Integration tests for the files/folders API endpoints:
//!   - `POST /api/files/upload` (multipart)
//!   - `GET /api/files`
//!   - `GET /api/files/:id`, `PATCH /api/files/:id`, `DELETE /api/files/:id`
//!   - `GET /api/files/:id/download`
//!   - `POST /api/files/batch/delete`
//!   - `GET /api/files/folders`, `POST /api/files/folders`,
//!     `GET/PATCH/DELETE /api/files/folders/:id`
//!
//! Auth notes (documented behavior):
//!   - Read/list/patch/batch-delete and folder delete have no permission gate
//!     and no actor write, so they run with the dev API key.
//!   - `upload_file` and `create_folder` record `uploaded_by`/`created_by`
//!     (`Uuid::nil()` when unauthenticated), which is an FK to `users(id)` —
//!     so writes require a real session-authenticated user (the dev key would
//!     otherwise violate the FK). These use an admin session.
//!   - `download_file` and `delete_file` call `check_file_permission`, which
//!     passes for an admin user (has the `users.all` scope) or a file linked to
//!     a collection. These use the admin session.
//!
//! `PATCH /api/files/:id` rename persists the storage path actually returned by
//! `file_storage.upload`, so a renamed file stays downloadable and deletable
//! (see `test_file_delete_after_rename`).

use axum::http::StatusCode;
use axum_test::multipart::{MultipartForm, Part};
use serde_json::{json, Value};
use uuid::Uuid;

#[path = "common/mod.rs"]
mod common;
use common::{setup, DEV_API_KEY};

const AUTH_HEADER: &str = "Bearer dev_test-key-for-tests-12345";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_body(response: &axum_test::TestResponse) -> Value {
    serde_json::from_str(&response.text()).expect("response body is valid JSON")
}

async fn get_authed(server: &axum_test::TestServer, path: &str) -> axum_test::TestResponse {
    server.get(path).add_header("Authorization", AUTH_HEADER).await
}

async fn post_authed(
    server: &axum_test::TestServer,
    path: &str,
    body: Value,
) -> axum_test::TestResponse {
    server
        .post(path)
        .add_header("Authorization", AUTH_HEADER)
        .json(&body)
        .await
}

async fn patch_authed(
    server: &axum_test::TestServer,
    path: &str,
    body: Value,
) -> axum_test::TestResponse {
    server
        .patch(path)
        .add_header("Authorization", AUTH_HEADER)
        .json(&body)
        .await
}

async fn delete_authed(server: &axum_test::TestServer, path: &str) -> axum_test::TestResponse {
    server
        .delete(path)
        .add_header("Authorization", AUTH_HEADER)
        .await
}

/// Upload a small text file (optionally into `folder_id`) as a session user.
/// A real session user is required because `uploaded_by` is an FK to `users`.
async fn upload_file(
    server: &axum_test::TestServer,
    cookie: &str,
    filename: &str,
    content: &[u8],
    folder_id: Option<&str>,
) -> axum_test::TestResponse {
    let mut form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes(content.to_vec())
                .file_name(filename)
                .mime_type("text/plain"),
        );
    if let Some(fid) = folder_id {
        form = form.add_text("folder_id", fid);
    }
    server
        .post("/api/files/upload")
        .add_header("cookie", cookie)
        .multipart(form)
        .await
}

/// Create a folder as a session user (same FK reason as upload).
async fn create_folder(
    server: &axum_test::TestServer,
    cookie: &str,
    name: &str,
) -> axum_test::TestResponse {
    server
        .post("/api/files/folders")
        .add_header("cookie", cookie)
        .json(&json!({ "name": name }))
        .await
}

/// Extract the live `alcedo_session=<id>` cookie value from a login response.
fn extract_session_cookie(resp: &axum_test::TestResponse) -> String {
    let mut found: Option<String> = None;
    for value in resp.headers().get_all("set-cookie") {
        for part in value.to_str().unwrap_or_default().split(';') {
            let part = part.trim();
            if let Some(cookie) = part.strip_prefix("alcedo_session=") {
                if !cookie.is_empty() {
                    found = Some(part.to_string());
                }
            }
        }
    }
    found.expect("login response should set a non-empty alcedo_session cookie")
}

/// Create a user, grant it the `users.all` scope via a role, and log in.
/// Returns the session cookie that authenticates as an admin.
async fn create_admin_session(server: &axum_test::TestServer) -> String {
    let email = format!("admin-{}@files.test", Uuid::new_v4().to_string().replace('-', "")[..8].to_string());
    let user = post_authed(
        server,
        "/api/users",
        json!({ "email": email, "password": "files-test-pw-1" }),
    )
    .await;
    assert_eq!(
        user.status_code(),
        StatusCode::OK,
        "create admin user failed: {}",
        user.text()
    );
    let user_id = parse_body(&user)["data"]["id"]
        .as_str()
        .expect("created user has an id")
        .to_string();

    let role = post_authed(
        server,
        "/api/roles",
        json!({ "name": format!("files-admin-{}", Uuid::new_v4().to_string().replace('-', "")[..8].to_string()) }),
    )
    .await;
    assert_eq!(role.status_code(), StatusCode::OK, "create role failed: {}", role.text());
    let role_id = parse_body(&role)["data"]["id"]
        .as_str()
        .expect("created role has an id")
        .to_string();

    let scopes = post_authed(
        server,
        &format!("/api/roles/{}/permissions", role_id),
        json!({ "permissions": ["users.all"] }),
    )
    .await;
    assert_eq!(
        scopes.status_code(),
        StatusCode::OK,
        "grant users.all scope failed: {}",
        scopes.text()
    );

    let assign = post_authed(
        server,
        &format!("/api/users/{}/roles", user_id),
        json!({ "role_id": role_id }),
    )
    .await;
    assert_eq!(
        assign.status_code(),
        StatusCode::OK,
        "assign role failed: {}",
        assign.text()
    );

    let login = server
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": "files-test-pw-1" }))
        .await;
    assert_eq!(login.status_code(), StatusCode::OK, "admin login failed: {}", login.text());
    extract_session_cookie(&login)
}

/// Minimal SHA-256 (FIPS 180-4) so tests don't need extra dependencies.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let o = i * 4;
            w[i] = u32::from_be_bytes([chunk[o], chunk[o + 1], chunk[o + 2], chunk[o + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|v| format!("{:08x}", v)).collect()
}

// ---------------------------------------------------------------------------
// Folders CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_folders_crud_round_trip() {
    let (server, _test_db) = setup().await;
    let cookie = create_admin_session(&server).await;

    // POST /api/files/folders → 200 with id + name.
    let name = common::unique_name("folder");
    let create = create_folder(&server, &cookie, &name).await;
    assert_eq!(
        create.status_code(),
        StatusCode::OK,
        "create folder failed: {}",
        create.text()
    );
    let body = parse_body(&create);
    let folder_id = body["id"].as_str().expect("created folder has an id").to_string();
    assert_eq!(body["name"].as_str(), Some(name.as_str()));

    // GET /api/files/folders lists it.
    let list = get_authed(&server, "/api/files/folders").await;
    assert_eq!(list.status_code(), StatusCode::OK, "list folders failed: {}", list.text());
    let data = parse_body(&list)["data"].as_array().expect("folders is an array").clone();
    assert!(
        data.iter().any(|f| f["id"].as_str() == Some(folder_id.as_str())),
        "created folder is present in the folders list"
    );

    // GET /api/files/folders/:id returns details.
    let single = get_authed(&server, &format!("/api/files/folders/{}", folder_id)).await;
    assert_eq!(single.status_code(), StatusCode::OK, "get folder failed: {}", single.text());
    let body = parse_body(&single);
    assert_eq!(body["name"].as_str(), Some(name.as_str()));
    assert_eq!(body["file_count"].as_i64(), Some(0), "empty folder has no files");
    assert_eq!(body["subfolder_count"].as_i64(), Some(0));

    // PATCH rename and read back.
    let renamed = format!("{}-renamed", name);
    let patch = patch_authed(
        &server,
        &format!("/api/files/folders/{}", folder_id),
        json!({ "name": renamed }),
    )
    .await;
    assert_eq!(patch.status_code(), StatusCode::OK, "rename folder failed: {}", patch.text());
    let body = parse_body(&patch);
    assert_eq!(body["name"].as_str(), Some(renamed.as_str()));

    let single = get_authed(&server, &format!("/api/files/folders/{}", folder_id)).await;
    assert_eq!(parse_body(&single)["name"].as_str(), Some(renamed.as_str()));

    // DELETE → 204, then GET → 404.
    let del = delete_authed(&server, &format!("/api/files/folders/{}", folder_id)).await;
    assert_eq!(del.status_code(), StatusCode::NO_CONTENT, "delete folder failed: {}", del.text());

    let gone = get_authed(&server, &format!("/api/files/folders/{}", folder_id)).await;
    assert_eq!(gone.status_code(), StatusCode::NOT_FOUND, "deleted folder is gone (404)");
}

// ---------------------------------------------------------------------------
// Upload → list → metadata → download → rename → delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_upload_list_metadata_download_delete_round_trip() {
    let (server, _test_db) = setup().await;
    let admin_cookie = create_admin_session(&server).await;

    let content = b"hello alcedo file storage\n".to_vec();
    let filename = "hello.txt";

    // Upload → 200 with id, size, sha256 matching the bytes.
    let upload = upload_file(&server, &admin_cookie, filename, &content, None).await;
    assert_eq!(upload.status_code(), StatusCode::OK, "upload failed: {}", upload.text());
    let body = parse_body(&upload);
    let file_id = body["id"].as_str().expect("uploaded file has an id").to_string();
    assert_eq!(body["filename"].as_str(), Some(filename));
    assert_eq!(body["size_bytes"].as_i64(), Some(content.len() as i64));
    assert_eq!(body["sha256"].as_str(), Some(sha256_hex(&content).as_str()));

    // GET /api/files lists it.
    let list = get_authed(&server, "/api/files").await;
    assert_eq!(list.status_code(), StatusCode::OK, "list files failed: {}", list.text());
    let data = parse_body(&list)["data"].as_array().expect("files is an array").clone();
    assert!(
        data.iter().any(|f| f["id"].as_str() == Some(file_id.as_str())),
        "uploaded file is present in the files list"
    );

    // GET /api/files/:id metadata matches.
    let meta = get_authed(&server, &format!("/api/files/{}", file_id)).await;
    assert_eq!(meta.status_code(), StatusCode::OK, "get file metadata failed: {}", meta.text());
    let body = parse_body(&meta);
    assert_eq!(body["filename"].as_str(), Some(filename));
    assert_eq!(body["size_bytes"].as_i64(), Some(content.len() as i64));
    assert_eq!(body["sha256"].as_str(), Some(sha256_hex(&content).as_str()));

    // GET /api/files/:id/download returns the exact bytes (admin session).
    let dl = server
        .get(&format!("/api/files/{}/download", file_id))
        .add_header("cookie", admin_cookie.clone())
        .await;
    assert_eq!(dl.status_code(), StatusCode::OK, "download failed: {}", dl.text());
    assert_eq!(
        dl.as_bytes().as_ref(),
        content.as_slice(),
        "downloaded bytes match the uploaded content"
    );

    // DELETE /api/files/:id → 204, then GET → 404.
    let del = server
        .delete(&format!("/api/files/{}", file_id))
        .add_header("cookie", admin_cookie.clone())
        .await;
    assert_eq!(del.status_code(), StatusCode::NO_CONTENT, "delete file failed: {}", del.text());
    let gone = get_authed(&server, &format!("/api/files/{}", file_id)).await;
    assert_eq!(gone.status_code(), StatusCode::NOT_FOUND, "deleted file is gone (404)");

    // PATCH /api/files/:id rename → read back.
    let rename_content = b"to be renamed".to_vec();
    let up2 = upload_file(&server, &admin_cookie, "rename-me.txt", &rename_content, None).await;
    assert_eq!(up2.status_code(), StatusCode::OK, "second upload failed: {}", up2.text());
    let file2_id = parse_body(&up2)["id"].as_str().expect("file2 id").to_string();

    let renamed = "renamed.txt";
    let patch = patch_authed(&server, &format!("/api/files/{}", file2_id), json!({ "filename": renamed })).await;
    assert_eq!(patch.status_code(), StatusCode::OK, "rename file failed: {}", patch.text());
    let meta = get_authed(&server, &format!("/api/files/{}", file2_id)).await;
    let body = parse_body(&meta);
    assert_eq!(body["filename"].as_str(), Some(renamed), "renamed file read back via GET");
    assert_eq!(body["size_bytes"].as_i64(), Some(rename_content.len() as i64));
}

// ---------------------------------------------------------------------------
// Batch delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_batch_delete_removes_files() {
    let (server, _test_db) = setup().await;
    let admin_cookie = create_admin_session(&server).await;

    let content = b"batch delete me".to_vec();
    let u1 = upload_file(&server, &admin_cookie, "b1.txt", &content, None).await;
    let u2 = upload_file(&server, &admin_cookie, "b2.txt", &content, None).await;
    assert_eq!(u1.status_code(), StatusCode::OK, "upload b1 failed: {}", u1.text());
    assert_eq!(u2.status_code(), StatusCode::OK, "upload b2 failed: {}", u2.text());
    let id1 = parse_body(&u1)["id"].as_str().expect("b1 id").to_string();
    let id2 = parse_body(&u2)["id"].as_str().expect("b2 id").to_string();

    // POST /api/files/batch/delete removes both.
    let batch = post_authed(&server, "/api/files/batch/delete", json!({ "ids": [id1, id2] })).await;
    assert_eq!(batch.status_code(), StatusCode::OK, "batch delete failed: {}", batch.text());
    let body = parse_body(&batch);
    assert_eq!(body["deleted"].as_i64(), Some(2), "both files deleted");
    assert_eq!(body["errors"].as_array().map(|a| a.len()), Some(0), "no delete errors");

    // GET by id → 404 for each.
    for id in [id1, id2] {
        let gone = get_authed(&server, &format!("/api/files/{}", id)).await;
        assert_eq!(gone.status_code(), StatusCode::NOT_FOUND, "file {} gone after batch delete", id);
    }
}

// ---------------------------------------------------------------------------
// Upload into a folder + folder delete guard
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_file_upload_in_folder_and_folder_delete_guard() {
    let (server, _test_db) = setup().await;
    let admin_cookie = create_admin_session(&server).await;

    let name = common::unique_name("docs");
    let folder = create_folder(&server, &admin_cookie, &name).await;
    assert_eq!(folder.status_code(), StatusCode::OK, "create folder failed: {}", folder.text());
    let folder_id = parse_body(&folder)["id"].as_str().expect("folder id").to_string();

    // Upload WITH folder_id → response associates the file with the folder.
    let content = b"file inside folder".to_vec();
    let upload = upload_file(&server, &admin_cookie, "notes.txt", &content, Some(&folder_id)).await;
    assert_eq!(upload.status_code(), StatusCode::OK, "upload to folder failed: {}", upload.text());
    let body = parse_body(&upload);
    let file_id = body["id"].as_str().expect("uploaded file has an id").to_string();
    assert_eq!(
        body["folder_id"].as_str(),
        Some(folder_id.as_str()),
        "uploaded file is associated with the folder"
    );

    // Metadata reflects the folder association.
    let meta = get_authed(&server, &format!("/api/files/{}", file_id)).await;
    assert_eq!(
        parse_body(&meta)["folder_id"].as_str(),
        Some(folder_id.as_str()),
        "file metadata keeps folder_id"
    );

    // Listing filtered by folder_id returns it.
    let list = get_authed(&server, &format!("/api/files?folder_id={}", folder_id)).await;
    let data = parse_body(&list)["data"].as_array().expect("files is an array").clone();
    assert!(
        data.iter().any(|f| f["id"].as_str() == Some(file_id.as_str())),
        "folder-scoped listing contains the uploaded file"
    );

    // Folder metadata reports the file count.
    let folder_meta = get_authed(&server, &format!("/api/files/folders/{}", folder_id)).await;
    assert_eq!(parse_body(&folder_meta)["file_count"].as_i64(), Some(1));

    // Non-recursive delete of a non-empty folder is rejected (400).
    let blocked = delete_authed(&server, &format!("/api/files/folders/{}", folder_id)).await;
    assert_eq!(
        blocked.status_code(),
        StatusCode::BAD_REQUEST,
        "non-empty folder delete must be rejected without recursive=true"
    );

    // Remove the file via batch delete, then the folder can be deleted.
    let batch = post_authed(&server, "/api/files/batch/delete", json!({ "ids": [file_id] })).await;
    assert_eq!(parse_body(&batch)["deleted"].as_i64(), Some(1));

    let del = delete_authed(&server, &format!("/api/files/folders/{}", folder_id)).await;
    assert_eq!(
        del.status_code(),
        StatusCode::NO_CONTENT,
        "folder delete after emptying: {}",
        del.text()
    );

    let gone = get_authed(&server, &format!("/api/files/folders/{}", folder_id)).await;
    assert_eq!(gone.status_code(), StatusCode::NOT_FOUND, "deleted folder is gone (404)");
}

// ---------------------------------------------------------------------------
// Rename round-trip: upload → rename → download → delete
// ---------------------------------------------------------------------------

/// `PATCH /api/files/:id` rename must persist the actual storage path returned
/// by `file_storage.upload`, so the renamed file is still downloadable with the
/// same bytes and deletable afterwards.
#[tokio::test]
async fn test_file_delete_after_rename_documents_bug() {
    let (server, _test_db) = setup().await;
    let admin_cookie = create_admin_session(&server).await;

    let content = b"delete-after-rename content".to_vec();
    let upload = upload_file(&server, &admin_cookie, "delete-after-rename.txt", &content, None).await;
    assert_eq!(upload.status_code(), StatusCode::OK, "upload failed: {}", upload.text());
    let file_id = parse_body(&upload)["id"].as_str().expect("file id").to_string();

    // Rename → 200, metadata reports the new name and same size.
    let patch = patch_authed(&server, &format!("/api/files/{}", file_id), json!({ "filename": "renamed.txt" })).await;
    assert_eq!(patch.status_code(), StatusCode::OK, "rename failed: {}", patch.text());
    let meta = get_authed(&server, &format!("/api/files/{}", file_id)).await;
    let body = parse_body(&meta);
    assert_eq!(body["filename"].as_str(), Some("renamed.txt"), "renamed file read back via GET");
    assert_eq!(body["size_bytes"].as_i64(), Some(content.len() as i64), "size preserved after rename");

    // Download after rename returns the exact same bytes.
    let dl = server
        .get(&format!("/api/files/{}/download", file_id))
        .add_header("cookie", admin_cookie.clone())
        .await;
    assert_eq!(dl.status_code(), StatusCode::OK, "download after rename failed: {}", dl.text());
    assert_eq!(
        dl.as_bytes().as_ref(),
        content.as_slice(),
        "downloaded bytes match the original uploaded content after rename"
    );

    // Delete after rename → 204, then GET → 404.
    let del = server
        .delete(&format!("/api/files/{}", file_id))
        .add_header("cookie", admin_cookie)
        .await;
    assert_eq!(del.status_code(), StatusCode::NO_CONTENT, "delete after rename failed: {}", del.text());
    let gone = get_authed(&server, &format!("/api/files/{}", file_id)).await;
    assert_eq!(gone.status_code(), StatusCode::NOT_FOUND, "deleted file is gone (404)");
}