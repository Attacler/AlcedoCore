//! Integration tests for the menus API:
//! `GET/POST /api/menus`, `GET/PUT/DELETE /api/menus/:id`,
//! `GET /api/menus/my`, `GET/PUT /api/menus/:id/roles`, `POST /api/menus/:id/copy`.

use plugin_core::api;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[path = "common/mod.rs"]
mod common;
use common::{create_test_session_layer, create_test_state_with_pool, TestDb, DEV_API_KEY};

async fn setup_server() -> (axum_test::TestServer, TestDb) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");
    (server, test_db)
}

async fn create_menu(server: &axum_test::TestServer, name: &str) -> Value {
    let payload = serde_json::json!({ "name": name });
    let resp = server
        .post("/api/menus")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&payload)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Create menu '{}' failed: {}",
        name,
        resp.text()
    );
    serde_json::from_str(&resp.text()).expect("Invalid JSON in create menu response")
}

async fn create_role(server: &axum_test::TestServer, name: &str) -> Value {
    let payload = serde_json::json!({ "name": name });
    let resp = server
        .post("/api/roles")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&payload)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Create role '{}' failed: {}",
        name,
        resp.text()
    );
    serde_json::from_str(&resp.text()).expect("Invalid JSON in create role response")
}

async fn create_user(server: &axum_test::TestServer, email: &str, password: &str) -> String {
    let payload = serde_json::json!({ "email": email, "password": password });
    let resp = server
        .post("/api/users")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&payload)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Create user '{}' failed: {}",
        email,
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in create user response");
    body["data"]["id"]
        .as_str()
        .expect("created user missing id")
        .to_string()
}

async fn login(server: &axum_test::TestServer, email: &str, password: &str) -> axum_test::TestResponse {
    server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await
}

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

fn test_section(label: &str) -> Value {
    serde_json::json!([
        {
            "id": "sec-1",
            "label": label,
            "icon": "folder",
            "visible": true,
            "items": [
                {
                    "id": "item-1",
                    "label": "Dashboard",
                    "icon": "home",
                    "visible": true,
                    "route": "/dashboard",
                    "sortOrder": 0
                }
            ]
        }
    ])
}

/// Set the menu's sections via `PUT /api/menus/:id` (the create endpoint only
/// stores name/icon — sections are written through the update handler).
async fn set_menu_sections(server: &axum_test::TestServer, id: &str, sections: Value) {
    let resp = server
        .put(&format!("/api/menus/{}", id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&serde_json::json!({ "sections": sections }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Set menu sections failed: {}",
        resp.text()
    );
}

#[tokio::test]
async fn test_create_menu_round_trip() {
    let (server, _test_db) = setup_server().await;

    let body = create_menu(&server, "Main Menu").await;
    let data = &body["data"];
    let id = data["id"]
        .as_str()
        .expect("created menu missing id")
        .to_string();
    assert!(!id.is_empty(), "created menu id should not be empty");
    assert_eq!(data["name"], "Main Menu", "created menu name matches");
    assert_eq!(data["icon"], "menu", "default icon is 'menu'");

    set_menu_sections(&server, &id, test_section("General")).await;

    let resp = server
        .get(&format!("/api/menus/{}", id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in get menu response");
    assert_eq!(
        body["data"]["sections"][0]["label"],
        "General",
        "menu section round-trips"
    );
    assert_eq!(
        body["data"]["sections"][0]["items"][0]["label"],
        "Dashboard",
        "menu item round-trips"
    );
}

#[tokio::test]
async fn test_list_menus_includes_created() {
    let (server, _test_db) = setup_server().await;

    create_menu(&server, "Listable Menu").await;

    let resp = server
        .get("/api/menus")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "List menus failed: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in list menus response");
    let menus = body["data"].as_array().expect("data should be an array");
    assert!(
        menus.iter().any(|m| m["name"] == "Listable Menu"),
        "created menu should appear in list, got: {:?}",
        menus
    );
}

#[tokio::test]
async fn test_get_menu_by_id() {
    let (server, _test_db) = setup_server().await;

    let created = create_menu(&server, "Get-by-id Menu").await;
    let id = created["data"]["id"].as_str().expect("missing menu id");

    let resp = server
        .get(&format!("/api/menus/{}", id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Get menu failed: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in get menu response");
    assert_eq!(body["data"]["id"], id, "returned menu id matches");
    assert_eq!(body["data"]["name"], "Get-by-id Menu", "returned menu name matches");
}

#[tokio::test]
async fn test_update_menu_rename() {
    let (server, _test_db) = setup_server().await;

    let created = create_menu(&server, "Original Name").await;
    let id = created["data"]["id"].as_str().expect("missing menu id");

    let payload = serde_json::json!({ "name": "Renamed Menu" });
    let resp = server
        .put(&format!("/api/menus/{}", id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&payload)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Update menu failed: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in update menu response");
    assert_eq!(body["data"]["id"], id);
    assert_eq!(body["data"]["name"], "Renamed Menu", "rename reflected in response");

    let resp = server
        .get(&format!("/api/menus/{}", id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in get menu response");
    assert_eq!(
        body["data"]["name"], "Renamed Menu",
        "rename reflected on read-back"
    );
}

#[tokio::test]
async fn test_menu_roles_round_trip() {
    let (server, _test_db) = setup_server().await;

    let menu = create_menu(&server, "Role Menu").await;
    let menu_id = menu["data"]["id"].as_str().expect("missing menu id");

    let role = create_role(&server, "menu-viewer").await;
    let role_id = role["data"]["id"].as_str().expect("missing role id");

    let resp = server
        .get(&format!("/api/menus/{}/roles", menu_id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Get menu roles failed: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in get menu roles response");
    assert_eq!(
        body["role_ids"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "new menu starts with no roles"
    );

    let resp = server
        .put(&format!("/api/menus/{}/roles", menu_id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&serde_json::json!({ "role_ids": [role_id] }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Set menu roles failed: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in set menu roles response");
    let ids: Vec<&str> = body["role_ids"]
        .as_array()
        .expect("role_ids should be an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(ids.contains(&role_id), "assigned role appears in response: {:?}", ids);

    let resp = server
        .get(&format!("/api/menus/{}/roles", menu_id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in get menu roles response");
    let ids: Vec<&str> = body["role_ids"]
        .as_array()
        .expect("role_ids should be an array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        ids.contains(&role_id),
        "assigned role reflected on read-back: {:?}",
        ids
    );
}

#[tokio::test]
async fn test_copy_menu_creates_new_menu_with_same_content() {
    let (server, _test_db) = setup_server().await;

    let source = create_menu(&server, "Source Menu").await;
    let source_id = source["data"]["id"].as_str().expect("missing source menu id").to_string();
    set_menu_sections(&server, &source_id, test_section("Copied Section")).await;

    let target = create_menu(&server, "Target Menu").await;
    let target_id = target["data"]["id"].as_str().expect("missing target menu id").to_string();
    assert_ne!(source_id, target_id, "source and target ids differ");

    let payload = serde_json::json!({ "source_menu_id": source_id });
    let resp = server
        .post(&format!("/api/menus/{}/copy", target_id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&payload)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Copy menu failed: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in copy menu response");
    let copied_id = body["data"]["id"].as_str().expect("copied menu missing id");
    assert_ne!(
        copied_id, source_id,
        "copy result is a different menu than the source"
    );
    assert_eq!(
        body["data"]["name"], "Target Menu",
        "copy keeps the target menu's own name"
    );
    assert_eq!(
        body["data"]["sections"][0]["label"], "Copied Section",
        "copy carries over the source menu's sections"
    );
    assert_eq!(
        body["data"]["sections"][0]["items"][0]["label"], "Dashboard",
        "copy carries over the source menu's items"
    );

    let resp = server
        .get(&format!("/api/menus/{}", source_id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in get source menu response");
    assert_eq!(
        body["data"]["sections"][0]["items"][0]["label"], "Dashboard",
        "source menu content is unchanged after copy"
    );
}

#[tokio::test]
async fn test_my_menus_requires_session_user() {
    let (server, _test_db) = setup_server().await;

    let email = format!("myuser{}@test.com", Uuid::new_v4());
    create_user(&server, &email, "test1234!").await;

    let login_resp = login(&server, &email, "test1234!").await;
    assert_eq!(login_resp.status_code(), axum::http::StatusCode::OK);
    let cookie = extract_session_cookie(&login_resp);

    let resp = server.get("/api/menus/my").add_header("cookie", cookie).await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "my menus should work with a session user: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in my menus response");
    assert_eq!(
        body["data"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "user with no roles sees no menus"
    );

    let resp = server
        .get("/api/menus/my")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::UNAUTHORIZED,
        "dev API key has no session user, so /api/menus/my must be 401: {}",
        resp.text()
    );
}

#[tokio::test]
async fn test_my_menus_lists_menus_for_visible_roles() {
    let (server, test_db) = setup_server().await;

    let menu = create_menu(&server, "Visible Menu").await;
    let menu_id = menu["data"]["id"].as_str().expect("missing menu id");

    let role = create_role(&server, "visible-menu-role").await;
    let role_id = role["data"]["id"].as_str().expect("missing role id");

    let email = format!("visibleuser{}@test.com", Uuid::new_v4());
    let user_id = create_user(&server, &email, "test1234!").await;

    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1::uuid, $2::uuid)"
    )
    .bind(&user_id)
    .bind(role_id)
    .execute(test_db.pool())
    .await
    .expect("failed to assign role to user");

    let login_resp = login(&server, &email, "test1234!").await;
    assert_eq!(login_resp.status_code(), axum::http::StatusCode::OK);
    let cookie = extract_session_cookie(&login_resp);

    let resp = server
        .get("/api/menus/my")
        .add_header("cookie", cookie.clone())
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "my menus failed: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in my menus response");
    let menus = body["data"].as_array().expect("data should be an array");
    assert!(
        menus.is_empty(),
        "menu not assigned to any role should not be visible: {:?}",
        menus
    );

    let resp = server
        .put(&format!("/api/menus/{}/roles", menu_id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&serde_json::json!({ "role_ids": [role_id] }))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);

    let resp = server
        .get("/api/menus/my")
        .add_header("cookie", cookie.clone())
        .await;
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in my menus response");
    let menus = body["data"].as_array().expect("data should be an array");
    assert!(
        menus.iter().any(|m| m["id"] == menu_id),
        "menu assigned to the user's role should be visible: {:?}",
        menus
    );
}

#[tokio::test]
async fn test_delete_menu_round_trip() {
    let (server, _test_db) = setup_server().await;

    let created = create_menu(&server, "Doomed Menu").await;
    let id = created["data"]["id"].as_str().expect("missing menu id");

    let resp = server
        .delete(&format!("/api/menus/{}", id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Delete menu failed: {}",
        resp.text()
    );
    let body: Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in delete menu response");
    assert_eq!(body["success"], true, "delete returns success");

    let resp = server
        .get(&format!("/api/menus/{}", id))
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::NOT_FOUND,
        "deleted menu should 404 on GET: {}",
        resp.text()
    );
}