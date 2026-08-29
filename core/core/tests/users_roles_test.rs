//! Integration tests for the users, roles, and user-roles API endpoints.
//!
//! Covers:
//!   - Users:      GET/POST /api/users, GET/PUT/DELETE /api/users/:id, POST /api/users/:id/password
//!   - Roles:      GET/POST /api/roles, GET/PUT/DELETE /api/roles/:id,
//!                 GET/POST /api/roles/:id/permissions, DELETE /api/roles/:id/permissions/:permission_id,
//!                 GET/POST /api/roles/:id/policies, DELETE /api/roles/:id/policies/:policy_id
//!   - User-Roles: POST /api/users/:id/roles, GET /api/users/:id/roles, DELETE /api/users/:id/roles/:role_id
//!
//! All requests authenticate with the provisioned dev API key
//! (`Authorization: Bearer dev_test-key-for-tests-12345`). In dev mode the
//! middleware treats a valid developer API key as an `AuthLevel::DeveloperApiKey`
//! and the handlers' `require_scope`/`check_permission` short-circuit, so the key
//! can exercise every endpoint without additional scopes.

use axum::http::StatusCode;
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

async fn authed_get(server: &axum_test::TestServer, path: &str) -> axum_test::TestResponse {
    server.get(path).add_header("Authorization", AUTH_HEADER).await
}

async fn authed_post(
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

async fn authed_put(
    server: &axum_test::TestServer,
    path: &str,
    body: Value,
) -> axum_test::TestResponse {
    server
        .put(path)
        .add_header("Authorization", AUTH_HEADER)
        .json(&body)
        .await
}

async fn authed_delete(server: &axum_test::TestServer, path: &str) -> axum_test::TestResponse {
    server
        .delete(path)
        .add_header("Authorization", AUTH_HEADER)
        .await
}

/// Create a user via the API and return the created user's `id` and `email`.
async fn create_user(
    server: &axum_test::TestServer,
    email: &str,
    password: &str,
) -> (String, String) {
    let resp = authed_post(
        server,
        "/api/users",
        json!({ "email": email, "password": password, "display_name": "Initial Name" }),
    )
    .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "create user failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    let id = body["data"]["id"]
        .as_str()
        .expect("created user has an id")
        .to_string();
    let returned_email = body["data"]["email"]
        .as_str()
        .expect("created user has an email")
        .to_string();
    assert_eq!(returned_email, email, "created user email matches");
    assert!(
        !body.to_string().contains("password_hash"),
        "create response must not expose password_hash"
    );
    (id, returned_email)
}

/// Create a role via the API and return the created role's `id` and `name`.
async fn create_role(
    server: &axum_test::TestServer,
    name: &str,
) -> (String, String) {
    let resp = authed_post(server, "/api/roles", json!({ "name": name })).await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "create role failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    let id = body["data"]["id"]
        .as_str()
        .expect("created role has an id")
        .to_string();
    let returned_name = body["data"]["name"]
        .as_str()
        .expect("created role has a name")
        .to_string();
    assert_eq!(returned_name, name, "created role name matches");
    (id, returned_name)
}

/// Create a policy via the API and return the created policy's `id`.
async fn create_policy(server: &axum_test::TestServer, name: &str) -> String {
    let resp = authed_post(
        server,
        "/api/policies",
        json!({ "name": name, "description": "policy for role test" }),
    )
    .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "create policy failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    body["id"].as_str().expect("created policy has an id").to_string()
}

fn unique_email() -> String {
    format!("user-{}@test.com", Uuid::new_v4().to_string().replace('-', "")[..12].to_string())
}

// ===========================================================================
// Users CRUD
// ===========================================================================

#[tokio::test]
async fn test_users_full_crud_round_trip() {
    let (server, _test_db) = setup().await;
    let email = unique_email();
    let (user_id, _) = create_user(&server, &email, "password123!").await;

    // GET list contains the created user.
    let list = authed_get(&server, "/api/users").await;
    assert_eq!(list.status_code(), StatusCode::OK, "list users failed");
    let body = parse_body(&list);
    let data = body["data"].as_array().expect("users list is an array");
    assert!(
        data.iter().any(|u| u["email"].as_str() == Some(email.as_str())),
        "created user is present in the users list"
    );
    assert!(
        !body.to_string().contains("password_hash"),
        "list users must not expose password_hash"
    );

    // GET by id returns the same email.
    let single = authed_get(&server, &format!("/api/users/{}", user_id)).await;
    assert_eq!(single.status_code(), StatusCode::OK, "get user failed");
    let body = parse_body(&single);
    assert_eq!(body["data"]["email"].as_str(), Some(email.as_str()));
    assert!(
        !body.to_string().contains("password_hash"),
        "get user must not expose password_hash"
    );

    // PUT rename (display_name) and read back.
    let rename = authed_put(
        &server,
        &format!("/api/users/{}", user_id),
        json!({ "display_name": "Renamed User" }),
    )
    .await;
    assert_eq!(rename.status_code(), StatusCode::OK, "rename user failed");
    let body = parse_body(&rename);
    assert_eq!(body["data"]["display_name"].as_str(), Some("Renamed User"));

    let single = authed_get(&server, &format!("/api/users/{}", user_id)).await;
    let body = parse_body(&single);
    assert_eq!(
        body["data"]["display_name"].as_str(),
        Some("Renamed User"),
        "renamed display_name read back via GET"
    );

    // DELETE then GET returns 404.
    let del = authed_delete(&server, &format!("/api/users/{}", user_id)).await;
    assert_eq!(del.status_code(), StatusCode::OK, "delete user failed");
    let body = parse_body(&del);
    assert_eq!(body["success"].as_bool(), Some(true));

    let gone = authed_get(&server, &format!("/api/users/{}", user_id)).await;
    assert_eq!(
        gone.status_code(),
        StatusCode::NOT_FOUND,
        "deleted user is gone (404)"
    );
}

// ===========================================================================
// Password change (POST /api/users/:id/password)
// ===========================================================================

#[tokio::test]
async fn test_password_change_invalidates_old_login() {
    let (server, _test_db) = setup().await;
    let email = unique_email();
    let (user_id, _) = create_user(&server, &email, "original-password").await;

    // Sanity: original password logs in.
    let login_ok = server
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": "original-password" }))
        .await;
    assert_eq!(
        login_ok.status_code(),
        StatusCode::OK,
        "original password logs in"
    );

    // Change password via the dev key (admin path in dev mode).
    let change = authed_post(
        &server,
        &format!("/api/users/{}/password", user_id),
        json!({
            "current_password": "original-password",
            "new_password": "brand-new-password",
        }),
    )
    .await;
    assert_eq!(
        change.status_code(),
        StatusCode::OK,
        "password change failed: {}",
        change.text()
    );
    let body = parse_body(&change);
    assert_eq!(body["success"].as_bool(), Some(true));

    // Old password now fails.
    let old_login = server
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": "original-password" }))
        .await;
    assert_eq!(
        old_login.status_code(),
        StatusCode::UNAUTHORIZED,
        "old password rejected after change"
    );

    // New password succeeds.
    let new_login = server
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": "brand-new-password" }))
        .await;
    assert_eq!(
        new_login.status_code(),
        StatusCode::OK,
        "new password logs in after change"
    );
}

#[tokio::test]
async fn test_password_change_rejects_wrong_current_password() {
    let (server, _test_db) = setup().await;
    let email = unique_email();
    let (user_id, _) = create_user(&server, &email, "correct-password").await;

    let change = authed_post(
        &server,
        &format!("/api/users/{}/password", user_id),
        json!({
            "current_password": "wrong-password",
            "new_password": "another-new-password",
        }),
    )
    .await;
    assert_eq!(
        change.status_code(),
        StatusCode::BAD_REQUEST,
        "wrong current password must be rejected: {}",
        change.text()
    );
}

// ===========================================================================
// Roles CRUD + permissions + policies
// ===========================================================================

#[tokio::test]
async fn test_roles_full_crud_round_trip() {
    let (server, _test_db) = setup().await;
    let role_name = format!("role-{}", Uuid::new_v4().to_string().replace('-', "")[..8].to_string());
    let (role_id, _) = create_role(&server, &role_name).await;

    // GET list contains the created role.
    let list = authed_get(&server, "/api/roles").await;
    assert_eq!(list.status_code(), StatusCode::OK, "list roles failed");
    let body = parse_body(&list);
    let data = body["data"].as_array().expect("roles list is an array");
    assert!(
        data.iter().any(|r| r["id"].as_str() == Some(role_id.as_str())),
        "created role is present in the roles list"
    );

    // GET by id.
    let single = authed_get(&server, &format!("/api/roles/{}", role_id)).await;
    assert_eq!(single.status_code(), StatusCode::OK, "get role failed");
    let body = parse_body(&single);
    assert_eq!(body["data"]["name"].as_str(), Some(role_name.as_str()));

    // PUT rename + read back.
    let renamed = format!("{}-renamed", role_name);
    let update = authed_put(
        &server,
        &format!("/api/roles/{}", role_id),
        json!({ "name": renamed, "description": "updated description" }),
    )
    .await;
    assert_eq!(update.status_code(), StatusCode::OK, "update role failed");
    let body = parse_body(&update);
    assert_eq!(body["data"]["name"].as_str(), Some(renamed.as_str()));

    let single = authed_get(&server, &format!("/api/roles/{}", role_id)).await;
    let body = parse_body(&single);
    assert_eq!(body["data"]["name"].as_str(), Some(renamed.as_str()));
    assert_eq!(body["data"]["description"].as_str(), Some("updated description"));

    // DELETE then GET returns 404.
    let del = authed_delete(&server, &format!("/api/roles/{}", role_id)).await;
    assert_eq!(del.status_code(), StatusCode::OK, "delete role failed");
    let body = parse_body(&del);
    assert_eq!(body["success"].as_bool(), Some(true));
    assert_eq!(body["deleted"].as_str(), Some(renamed.as_str()));

    let gone = authed_get(&server, &format!("/api/roles/{}", role_id)).await;
    assert_eq!(gone.status_code(), StatusCode::NOT_FOUND, "deleted role is gone (404)");
}

#[tokio::test]
async fn test_role_permissions_add_list_delete() {
    let (server, _test_db) = setup().await;
    let role_name = format!("perm-role-{}", Uuid::new_v4().to_string().replace('-', "")[..8].to_string());
    let (role_id, _) = create_role(&server, &role_name).await;

    // Initially empty.
    let initial = authed_get(&server, &format!("/api/roles/{}/permissions", role_id)).await;
    assert_eq!(initial.status_code(), StatusCode::OK);
    let body = parse_body(&initial);
    assert_eq!(body["data"].as_array().map(|a| a.len()), Some(0), "role starts with no scopes");

    // POST replaces the scope set with two scopes.
    let set = authed_post(
        &server,
        &format!("/api/roles/{}/permissions", role_id),
        json!({ "permissions": ["kv.read", "items.read"] }),
    )
    .await;
    assert_eq!(set.status_code(), StatusCode::OK, "set permissions failed: {}", set.text());
    let body = parse_body(&set);
    let data = body["data"].as_array().expect("permissions is an array");
    assert_eq!(data.len(), 2, "two scopes assigned");
    let scopes: Vec<String> = data
        .iter()
        .map(|p| p["scope"].as_str().expect("scope is a string").to_string())
        .collect();
    assert!(scopes.contains(&"kv.read".to_string()));
    assert!(scopes.contains(&"items.read".to_string()));
    let perm_ids: Vec<String> = data
        .iter()
        .map(|p| p["id"].as_str().expect("permission row has an id").to_string())
        .collect();

    // GET lists the assigned scopes.
    let list = authed_get(&server, &format!("/api/roles/{}/permissions", role_id)).await;
    let body = parse_body(&list);
    let data = body["data"].as_array().expect("permissions is an array");
    assert_eq!(data.len(), 2);

    // DELETE one permission by id, then GET shows only the remaining one.
    let del = authed_delete(
        &server,
        &format!("/api/roles/{}/permissions/{}", role_id, perm_ids[0]),
    )
    .await;
    assert_eq!(del.status_code(), StatusCode::OK, "delete permission failed");
    let body = parse_body(&del);
    assert_eq!(body["success"].as_bool(), Some(true));

    let list = authed_get(&server, &format!("/api/roles/{}/permissions", role_id)).await;
    let body = parse_body(&list);
    let data = body["data"].as_array().expect("permissions is an array");
    assert_eq!(data.len(), 1, "one scope remains after delete");
    assert_eq!(data[0]["id"].as_str(), Some(perm_ids[1].as_str()));
}

#[tokio::test]
async fn test_role_policies_assign_list_remove() {
    let (server, _test_db) = setup().await;
    let role_name = format!("pol-role-{}", Uuid::new_v4().to_string().replace('-', "")[..8].to_string());
    let (role_id, _) = create_role(&server, &role_name).await;
    let policy_id = create_policy(&server, &format!("policy-{}", role_name)).await;

    // Initially empty.
    let initial = authed_get(&server, &format!("/api/roles/{}/policies", role_id)).await;
    assert_eq!(initial.status_code(), StatusCode::OK);
    let body = parse_body(&initial);
    assert_eq!(body["data"].as_array().map(|a| a.len()), Some(0));

    // Assign the policy.
    let assign = authed_post(
        &server,
        &format!("/api/roles/{}/policies", role_id),
        json!({ "policy_id": policy_id }),
    )
    .await;
    assert_eq!(assign.status_code(), StatusCode::OK, "assign policy failed: {}", assign.text());
    let body = parse_body(&assign);
    assert_eq!(body["success"].as_bool(), Some(true));

    // GET lists it.
    let list = authed_get(&server, &format!("/api/roles/{}/policies", role_id)).await;
    assert_eq!(list.status_code(), StatusCode::OK);
    let body = parse_body(&list);
    let data = body["data"].as_array().expect("policies is an array");
    assert_eq!(data.len(), 1, "assigned policy listed");
    assert_eq!(data[0]["id"].as_str(), Some(policy_id.as_str()));

    // Remove it.
    let remove = authed_delete(
        &server,
        &format!("/api/roles/{}/policies/{}", role_id, policy_id),
    )
    .await;
    assert_eq!(remove.status_code(), StatusCode::OK, "remove policy failed");
    let body = parse_body(&remove);
    assert_eq!(body["success"].as_bool(), Some(true));

    // GET shows it gone.
    let list = authed_get(&server, &format!("/api/roles/{}/policies", role_id)).await;
    let body = parse_body(&list);
    assert_eq!(
        body["data"].as_array().map(|a| a.len()),
        Some(0),
        "policy removed from role"
    );
}

// ===========================================================================
// User-Roles
// ===========================================================================

#[tokio::test]
async fn test_user_role_assign_list_remove() {
    let (server, _test_db) = setup().await;
    let email = unique_email();
    let (user_id, _) = create_user(&server, &email, "password123!").await;
    let role_name = format!("ur-role-{}", Uuid::new_v4().to_string().replace('-', "")[..8].to_string());
    let (role_id, _) = create_role(&server, &role_name).await;

    // Initially no roles assigned.
    let initial = authed_get(&server, &format!("/api/users/{}/roles", user_id)).await;
    assert_eq!(initial.status_code(), StatusCode::OK, "list user roles failed");
    let body = parse_body(&initial);
    assert_eq!(body["data"].as_array().map(|a| a.len()), Some(0));

    // Assign role (body must contain role_id).
    let assign = authed_post(
        &server,
        &format!("/api/users/{}/roles", user_id),
        json!({ "role_id": role_id }),
    )
    .await;
    assert_eq!(assign.status_code(), StatusCode::OK, "assign role failed: {}", assign.text());
    let body = parse_body(&assign);
    assert_eq!(body["success"].as_bool(), Some(true));

    // GET lists the assigned role.
    let list = authed_get(&server, &format!("/api/users/{}/roles", user_id)).await;
    assert_eq!(list.status_code(), StatusCode::OK);
    let body = parse_body(&list);
    let data = body["data"].as_array().expect("user roles is an array");
    assert_eq!(data.len(), 1, "one role assigned");
    assert_eq!(data[0]["id"].as_str(), Some(role_id.as_str()));
    assert_eq!(data[0]["name"].as_str(), Some(role_name.as_str()));

    // Remove the assignment.
    let remove = authed_delete(
        &server,
        &format!("/api/users/{}/roles/{}", user_id, role_id),
    )
    .await;
    assert_eq!(remove.status_code(), StatusCode::OK, "remove role failed");
    let body = parse_body(&remove);
    assert_eq!(body["success"].as_bool(), Some(true));

    // GET shows it gone.
    let list = authed_get(&server, &format!("/api/users/{}/roles", user_id)).await;
    let body = parse_body(&list);
    assert_eq!(
        body["data"].as_array().map(|a| a.len()),
        Some(0),
        "role assignment removed"
    );
}