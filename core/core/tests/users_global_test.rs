//! Regression test: the global `/api/users` API must work without an app schema.
//!
//! On a fresh boot only the global `alcedo` schema exists — there is no
//! `default010v1` app schema and therefore no `alcedocore_collection_definitions`
//! table. The users handlers must operate directly on the global
//! `alcedo.alcedo_users` table instead of the per-app collection machinery.

use serde_json::Value;

#[path = "common/mod.rs"]
mod common;
use common::{setup, DEV_API_KEY};

/// Drop the app schema to simulate a fresh boot with no app context.
async fn drop_app_schema(pool: &sqlx::PgPool) {
    sqlx::query(r#"DROP SCHEMA IF EXISTS "default010v1" CASCADE"#)
        .execute(pool)
        .await
        .expect("failed to drop default010v1 schema");
}

async fn assert_no_app_schema(pool: &sqlx::PgPool) {
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'default010v1')"#,
    )
    .fetch_one(pool)
    .await
    .expect("schema existence query failed");
    assert!(!exists, "the app schema should not exist for this test");
}

/// Extract the live `alcedo_session=<id>` cookie from a login response.
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

/// Regression: with no app schema, `require_admin` must fail closed (403)
/// instead of returning a 500 when its app-scoped `users.all` query hits a
/// missing relation (`alcedocore_user_roles`).
#[tokio::test]
async fn non_admin_apps_api_forbidden_without_app_schema() {
    let (server, test_db) = setup().await;
    drop_app_schema(test_db.pool()).await;
    assert_no_app_schema(test_db.pool()).await;

    let resp = server
        .post("/api/users")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&serde_json::json!({
            "email": "plain@example.com",
            "password": "test1234!",
            "is_admin": false
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "create non-admin without app schema failed: {}",
        resp.text()
    );

    let login = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "email": "plain@example.com", "password": "test1234!" }))
        .await;
    assert_eq!(
        login.status_code(),
        axum::http::StatusCode::OK,
        "login failed: {}",
        login.text()
    );
    let cookie = extract_session_cookie(&login);

    let resp = server.get("/api/apps").add_header("cookie", &cookie).await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::FORBIDDEN,
        "non-admin GET /api/apps without app schema should be 403, got {}: {}",
        resp.status_code(),
        resp.text()
    );
}

#[tokio::test]
async fn users_api_works_without_app_schema() {
    let (server, test_db) = setup().await;
    drop_app_schema(test_db.pool()).await;
    assert_no_app_schema(test_db.pool()).await;

    let auth = format!("Bearer {}", DEV_API_KEY);

    // ---- CREATE ---------------------------------------------------------
    let resp = server
        .post("/api/users")
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({
            "email": "fresh@example.com",
            "password": "test1234!",
            "display_name": "Fresh User",
            "is_admin": false
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "create user without app schema failed: {}",
        resp.text()
    );
    let body: Value = serde_json::from_str(&resp.text()).expect("create body is JSON");
    assert_eq!(body["data"]["email"], "fresh@example.com");
    assert_eq!(body["data"]["display_name"], "Fresh User");
    assert_eq!(body["data"]["is_admin"], false);
    assert!(
        body["data"].get("password_hash").is_none(),
        "create response must not expose password_hash: {}",
        resp.text()
    );
    let id = body["data"]["id"]
        .as_str()
        .expect("created user id")
        .to_string();

    // ---- LIST -----------------------------------------------------------
    let resp = server
        .get("/api/users")
        .add_header("Authorization", auth.clone())
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "list users without app schema failed: {}",
        resp.text()
    );
    let body: Value = serde_json::from_str(&resp.text()).expect("list body is JSON");
    let data = body["data"].as_array().expect("list data is an array");
    let listed = data
        .iter()
        .find(|u| u["email"] == "fresh@example.com")
        .expect("created user is present in list");
    assert!(
        listed["$permissions"]["update"].as_bool().unwrap_or(false),
        "admin user should report update permission: {listed}"
    );
    assert!(
        listed["$permissions"]["delete"].as_bool().unwrap_or(false),
        "admin user should report delete permission: {listed}"
    );
    assert!(
        data.iter().all(|u| u.get("password_hash").is_none()),
        "list response must not expose password_hash: {}",
        resp.text()
    );

    // ---- GET ------------------------------------------------------------
    let resp = server
        .get(&format!("/api/users/{id}"))
        .add_header("Authorization", auth.clone())
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "get user without app schema failed: {}",
        resp.text()
    );
    let body: Value = serde_json::from_str(&resp.text()).expect("get body is JSON");
    assert_eq!(body["data"]["id"], id);
    assert!(body["data"].get("password_hash").is_none());

    // ---- UPDATE ---------------------------------------------------------
    let resp = server
        .put(&format!("/api/users/{id}"))
        .add_header("Authorization", auth.clone())
        .json(&serde_json::json!({ "display_name": "Renamed User" }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "update user without app schema failed: {}",
        resp.text()
    );
    let body: Value = serde_json::from_str(&resp.text()).expect("update body is JSON");
    assert_eq!(body["data"]["display_name"], "Renamed User");
    assert!(body["data"].get("password_hash").is_none());

    // ---- DELETE ---------------------------------------------------------
    let resp = server
        .delete(&format!("/api/users/{id}"))
        .add_header("Authorization", auth.clone())
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "delete user without app schema failed: {}",
        resp.text()
    );

    let resp = server
        .get(&format!("/api/users/{id}"))
        .add_header("Authorization", auth)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::NOT_FOUND,
        "deleted user should be gone: {}",
        resp.text()
    );
}
