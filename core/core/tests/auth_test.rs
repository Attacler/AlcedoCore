//! Integration tests for the auth API endpoints:
//! `POST /api/auth/login`, `GET /api/auth/me`, and `POST /api/auth/logout`.
//!
//! Session cookies are handled manually: login responses carry a `set-cookie`
//! header (`alcedo_session=<id>`), and we echo it back as a `cookie` header on
//! subsequent requests because `axum_test` does not persist cookies across calls.

use serde_json::Value;

#[path = "common/mod.rs"]
mod common;
use common::{setup, DEV_API_KEY};

const TEST_EMAIL: &str = "test@auth.com";
const TEST_PASSWORD: &str = "test1234!";

/// Create the test user via `POST /api/users` (Bearer dev key).
///
/// In dev mode `check_permission` bypasses when there's no plugin identity
/// (no `X-Request-ID`), so the dev API key can create users directly.
async fn create_test_user(server: &axum_test::TestServer) {
    let payload = serde_json::json!({
        "email": TEST_EMAIL,
        "password": TEST_PASSWORD,
    });
    let resp = server
        .post("/api/users")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&payload)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "create user failed: {}",
        resp.text()
    );
}

/// Attempt a login and return the raw response.
async fn login(
    server: &axum_test::TestServer,
    email: &str,
    password: &str,
) -> axum_test::TestResponse {
    server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await
}

/// Extract the live `alcedo_session=<id>` cookie value from a login response.
///
/// tower-sessions may emit several `set-cookie` headers while rotating the
/// session; the last one carrying a non-empty `alcedo_session=` value is the
/// active session cookie to send back.
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

// ===========================================================================
// POST /api/auth/login
// ===========================================================================

#[tokio::test]
async fn test_login_success_returns_user() {
    let (server, _test_db) = setup().await;
    create_test_user(&server).await;

    let resp = login(&server, TEST_EMAIL, TEST_PASSWORD).await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "login should succeed: {}",
        resp.text()
    );

    let body: Value = serde_json::from_str(&resp.text()).expect("login body is valid JSON");
    assert_eq!(
        body["user"]["email"].as_str(),
        Some(TEST_EMAIL),
        "returned user email matches"
    );
    assert!(
        body["user"]["id"].as_str().is_some(),
        "returned user has an id"
    );
    // Successful login must set a session cookie.
    assert!(
        resp.headers().contains_key("set-cookie"),
        "login response should set a session cookie"
    );
}

#[tokio::test]
async fn test_login_wrong_password_rejected() {
    let (server, _test_db) = setup().await;
    create_test_user(&server).await;

    let resp = login(&server, TEST_EMAIL, "wrong-password").await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::UNAUTHORIZED,
        "wrong password must be rejected (401): {}",
        resp.text()
    );
}

#[tokio::test]
async fn test_login_unknown_email_rejected() {
    let (server, _test_db) = setup().await;

    let resp = login(&server, "nobody@example.com", "whatever123!").await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::UNAUTHORIZED,
        "unknown email must be rejected (401): {}",
        resp.text()
    );
}

// ===========================================================================
// GET /api/auth/me
// ===========================================================================

#[tokio::test]
async fn test_me_with_session_cookie_returns_user_and_scopes() {
    let (server, _test_db) = setup().await;
    create_test_user(&server).await;

    let login_resp = login(&server, TEST_EMAIL, TEST_PASSWORD).await;
    assert_eq!(login_resp.status_code(), axum::http::StatusCode::OK);
    let cookie = extract_session_cookie(&login_resp);

    let resp = server
        .get("/api/auth/me")
        .add_header("cookie", cookie)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "me should succeed with a valid session cookie: {}",
        resp.text()
    );

    let body: Value = serde_json::from_str(&resp.text()).expect("me body is valid JSON");
    assert_eq!(
        body["user"]["email"].as_str(),
        Some(TEST_EMAIL),
        "me returns the logged-in user"
    );
    assert!(
        body["scopes"].is_array(),
        "scopes should be an array, got: {}",
        body["scopes"]
    );
}

#[tokio::test]
async fn test_me_without_cookie_is_unauthorized() {
    let (server, _test_db) = setup().await;

    let resp = server.get("/api/auth/me").await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::UNAUTHORIZED,
        "me without a session cookie must be 401: {}",
        resp.text()
    );
}

// ===========================================================================
// POST /api/auth/logout
// ===========================================================================

#[tokio::test]
async fn test_logout_invalidates_session() {
    let (server, _test_db) = setup().await;
    create_test_user(&server).await;

    let login_resp = login(&server, TEST_EMAIL, TEST_PASSWORD).await;
    assert_eq!(login_resp.status_code(), axum::http::StatusCode::OK);
    let cookie = extract_session_cookie(&login_resp);

    let logout_resp = server
        .post("/api/auth/logout")
        .add_header("cookie", cookie.clone())
        .await;
    assert_eq!(
        logout_resp.status_code(),
        axum::http::StatusCode::OK,
        "logout should succeed: {}",
        logout_resp.text()
    );
    let body: Value =
        serde_json::from_str(&logout_resp.text()).expect("logout body is valid JSON");
    assert_eq!(body["success"].as_bool(), Some(true));

    // The session is gone server-side; the same cookie must no longer authenticate.
    let me_resp = server.get("/api/auth/me").add_header("cookie", cookie).await;
    assert_eq!(
        me_resp.status_code(),
        axum::http::StatusCode::UNAUTHORIZED,
        "me with a logged-out session cookie must be 401: {}",
        me_resp.text()
    );
}