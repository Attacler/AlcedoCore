//! Integration tests for the auth API endpoints:
//! `POST /api/auth/login`, `GET /api/auth/me`, and `POST /api/auth/logout`.
//!
//! Session cookies are handled manually: login responses carry a `set-cookie`
//! header (`alcedo_session=<id>`), and we echo it back as a `cookie` header on
//! subsequent requests because `axum_test` does not persist cookies across calls.

use serde_json::Value;
use std::sync::Arc;

#[path = "common/mod.rs"]
mod common;
use common::{setup, with_default_app_headers, DEV_API_KEY};

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

/// Build a test server whose `AppState` has a real Redis client wired in so the
/// login lockout counter (`login_fail:{email}`) is actually enforced. The dev API
/// key is provisioned so requests carrying it are privileged and the error-detail
/// sanitizer preserves the lockout message in the response body.
async fn setup_with_redis() -> (axum_test::TestServer, common::TestDb) {
    let test_db = common::TestDb::new()
        .await
        .expect("Failed to create test DB");
    let mut state = common::create_test_state_with_pool(test_db.pool().clone()).await;
    state.redis = Some(Arc::new(common::connect_redis_client().await));
    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    )
    .await;
    let session_layer = common::create_test_session_layer().await;
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(with_default_app_headers(app)).expect("Failed to create test server");
    (server, test_db)
}

// ===========================================================================
// POST /api/auth/login — brute-force lockout
// ===========================================================================

#[tokio::test]
async fn test_login_lockout_after_five_failures() {
    let (server, _test_db) = setup_with_redis().await;

    // Use a nonexistent email and an isolated client IP so neither the
    // email-based lockout nor the IP rate limiter is shared with other tests.
    let email = format!("lockout-{}@example.com", uuid::Uuid::new_v4());
    let client_ip = format!(
        "198.51.100.{}",
        (uuid::Uuid::new_v4().as_u128() % 200) as u8 + 1
    );

    // Five wrong-password attempts: all 401, each recorded as a failure.
    for attempt in 1..=5 {
        let resp = server
            .post("/api/auth/login")
            .add_header("x-forwarded-for", client_ip.as_str())
            .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
            .json(&serde_json::json!({ "email": email, "password": "wrong-password" }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::UNAUTHORIZED,
            "attempt {} should be rejected (401): {}",
            attempt,
            resp.text()
        );
    }

    // Sixth attempt is rejected by the lockout before authentication.
    let resp = server
        .post("/api/auth/login")
        .add_header("x-forwarded-for", client_ip.as_str())
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .json(&serde_json::json!({ "email": email, "password": "wrong-password" }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        "6th attempt should be locked out (429): {}",
        resp.text()
    );
    let body: Value = serde_json::from_str(&resp.text()).expect("lockout body is valid JSON");
    assert!(
        body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("Too many login attempts"),
        "6th attempt should report the login lockout, not an IP rate limit: {}",
        resp.text()
    );

    // Clean up so the counter does not leak across test runs.
    let redis = common::connect_redis_client().await;
    redis
        .del(&format!("login_fail:{}", email))
        .await
        .expect("failed to clean up lockout key");
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
    assert!(
        body["is_admin"].is_boolean(),
        "is_admin should be a boolean, got: {}",
        body["is_admin"]
    );
}

/// Seed `default`/`v1` in the global app/version tables so the context
/// middleware can resolve the `default010v1` schema for `X-App`/`X-Version`.
/// Idempotent — the dev key provisioning in `setup()` already seeds the rows.
async fn seed_default_app_version(pool: &sqlx::PgPool) {
    let _ = plugin_core::services::auth::ensure_default_app_version(pool)
        .await
        .expect("failed to seed default app/version");
}

/// `GET /api/auth/me` scopes depend on the resolved app-version context:
/// no `X-App`/`X-Version` resolves the default/production (global) zone, so
/// scopes are empty; `X-App: default` + `X-Version: v1` resolves the
/// `default010v1` schema and returns the user's role scopes from it.
#[tokio::test]
async fn auth_me_scopes_are_context_aware() {
    let (server, test_db) = setup().await;
    create_test_user(&server).await;

    let login_resp = login(&server, TEST_EMAIL, TEST_PASSWORD).await;
    assert_eq!(login_resp.status_code(), axum::http::StatusCode::OK);
    let cookie = extract_session_cookie(&login_resp);
    let login_body: Value =
        serde_json::from_str(&login_resp.text()).expect("login body is valid JSON");
    let user_id = uuid::Uuid::parse_str(
        login_body["user"]["id"]
            .as_str()
            .expect("login body has user id"),
    )
    .expect("user id is a valid UUID");
    let user_is_admin = login_body["user"]["is_admin"]
        .as_bool()
        .expect("login body has is_admin");

    // Register default/v1 so the context middleware resolves the schema.
    seed_default_app_version(test_db.pool()).await;

    // Grant a scope through the app-version role tables in `default010v1`.
    let role_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO alcedocore_roles (id, name, is_system)
           VALUES ($1, 'auth-me-context-test', false)"#,
    )
    .bind(role_id)
    .execute(test_db.pool())
    .await
    .expect("failed to create role");
    sqlx::query(
        r#"INSERT INTO alcedocore_role_scopes (role_id, scope) VALUES ($1, 'roles.read')"#,
    )
    .bind(role_id)
    .execute(test_db.pool())
    .await
    .expect("failed to grant scope to role");
    sqlx::query(r#"INSERT INTO alcedocore_user_roles (user_id, role_id) VALUES ($1, $2)"#)
        .bind(user_id)
        .bind(role_id)
        .execute(test_db.pool())
        .await
        .expect("failed to assign role to user");

    // An unresolvable app/version context → global zone → no scopes. (The test
    // harness injects default `X-App: default`/`X-Version: v1` on every request,
    // so an unknown pair is used to force the global zone instead.)
    let me_resp = server
        .get("/api/auth/me")
        .add_header("cookie", cookie.clone())
        .add_header("X-App", "does-not-exist")
        .add_header("X-Version", "v1")
        .await;
    assert_eq!(
        me_resp.status_code(),
        axum::http::StatusCode::OK,
        "me should succeed: {}",
        me_resp.text()
    );
    let me_body: Value = serde_json::from_str(&me_resp.text()).expect("me body is valid JSON");
    assert_eq!(
        me_body["scopes"],
        serde_json::json!([]),
        "an unresolvable app/version puts the user in the global zone with no scopes"
    );
    assert_eq!(
        me_body["is_admin"].as_bool(),
        Some(user_is_admin),
        "is_admin reflects the user record"
    );

    // Explicit app/version context → resolves `default010v1` → role scopes.
    let me_resp = server
        .get("/api/auth/me")
        .add_header("cookie", cookie.clone())
        .add_header("X-App", "default")
        .add_header("X-Version", "v1")
        .await;
    assert_eq!(
        me_resp.status_code(),
        axum::http::StatusCode::OK,
        "me should succeed: {}",
        me_resp.text()
    );
    let me_body: Value = serde_json::from_str(&me_resp.text()).expect("me body is valid JSON");
    assert_eq!(
        me_body["scopes"],
        serde_json::json!(["roles.read"]),
        "X-App/X-Version resolves the default010v1 schema and returns the role scope"
    );
    assert_eq!(
        me_body["is_admin"].as_bool(),
        Some(user_is_admin),
        "is_admin reflects the user record"
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