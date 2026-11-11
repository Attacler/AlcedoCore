use alcedo_sdk::*;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param, header, header_exists},
};
use serde_json::{json, Value};

/// Helper to create a client pointed at a wiremock server
async fn test_client(server: &MockServer) -> AlcedoClient {
    AlcedoClientBuilder::new()
        .base_url(server.uri())
        .plugin_slug("test-plugin")
        .build()
        .expect("Failed to build test client")
}

/// JSend success envelope, as returned by the core API.
fn jsend(data: Value) -> Value {
    json!({ "status": "success", "data": data })
}

async fn mock_json(server: &MockServer, m: &str, p: &str, body: Value) {
    Mock::given(method(m))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mock_jsend(server: &MockServer, m: &str, p: &str, data: Value) {
    mock_json(server, m, p, jsend(data)).await;
}

// ─── Client behavior: JSend unwrapping, headers, errors ─────────────────────

#[tokio::test]
async fn test_jsend_success_unwraps_data() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/app/kv/my-key", json!({ "key": "my-key", "value": "v" })).await;

    let client = test_client(&server).await;
    let value = client.kv.get("my-key").await.unwrap();
    assert_eq!(value, Some(json!({ "key": "my-key", "value": "v" })));
}

#[tokio::test]
async fn test_jsend_fail_raises_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/app/kv/bad"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "status": "fail", "message": "Invalid key", "code": 1001
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    match client.kv.get("bad").await {
        Err(AlcedoError::Api { message, code, status, http_status }) => {
            assert_eq!(message, "Invalid key");
            assert_eq!(code.as_deref(), Some("1001"));
            assert_eq!(status, "fail");
            assert_eq!(http_status, Some(400));
        }
        other => panic!("Expected Api error, got {:?}", other),
    }
}

#[tokio::test]
async fn test_jsend_error_envelope_on_500() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/platform/logs"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "status": "error", "message": "db down"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    match client.activity_logs.list_system(None).await {
        Err(AlcedoError::Api { message, status, .. }) => {
            assert_eq!(message, "db down");
            assert_eq!(status, "error");
        }
        other => panic!("Expected Api error, got {:?}", other),
    }
}

#[tokio::test]
async fn test_non_jsend_body_passes_through() {
    let server = MockServer::start().await;
    // POST /api/dev/request-id returns a bare {request_id} — not JSend.
    mock_json(&server, "POST", "/api/dev/request-id", json!({ "request_id": "req-123" })).await;

    let client = test_client(&server).await;
    let resp = client.dev.request_id("test-plugin").await.unwrap();
    assert_eq!(resp.request_id, "req-123");
}

#[tokio::test]
async fn test_auth_app_version_headers_injected() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/app/kv/k"))
        .and(header("Authorization", "Bearer secret-key"))
        .and(header("X-App", "my-app"))
        .and(header("X-Version", "production"))
        .and(header_exists("X-Request-ID"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jsend(json!({ "key": "k", "value": 1 }))))
        .mount(&server)
        .await;

    let client = AlcedoClientBuilder::new()
        .base_url(server.uri())
        .api_key("secret-key")
        .app("my-app")
        .version("production")
        .build()
        .unwrap();
    client.kv.get("k").await.unwrap();
}

#[tokio::test]
async fn test_request_with_context_per_call_override() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/app/settings"))
        .and(header("X-App", "other-app"))
        .and(header("X-Version", "staging"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jsend(json!({})))
            .set_body_json(jsend(json!({ "site_name": "x" }))))
        .mount(&server)
        .await;

    let base = BaseClient::with_context(
        &server.uri(),
        "test-plugin",
        std::time::Duration::from_secs(5),
        None,
        None,
        Some("my-app".to_string()),
        Some("production".to_string()),
    )
    .unwrap();
    let resp: Value = base
        .execute_json(base.request_with_context(reqwest::Method::GET, "/api/app/settings", Some("other-app"), Some("staging")))
        .await
        .unwrap();
    assert_eq!(resp, json!({ "site_name": "x" }));
}

#[tokio::test]
async fn test_error_status_mapping() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/app/kv/secret"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "message": "unauthorized" })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    match client.kv.get("secret").await {
        Err(AlcedoError::Authentication { status_code, .. }) => assert_eq!(status_code, 401),
        other => panic!("Expected Authentication error, got {:?}", other),
    }
}

// ─── health ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_health_check() {
    let server = MockServer::start().await;
    mock_json(&server, "GET", "/health", json!({
        "status": "ok",
        "core": { "db": "connected", "docker": "swarm" },
        "plugins": []
    })).await;

    let client = test_client(&server).await;
    let health = client.health.check().await.unwrap();
    assert_eq!(health.status, "ok");
    assert_eq!(health.core.db, "connected");
}

// ─── kv ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_kv_set_and_delete() {
    let server = MockServer::start().await;
    mock_jsend(&server, "PUT", "/api/app/kv/my-key", json!({ "key": "my-key", "value": "hello" })).await;
    mock_jsend(&server, "DELETE", "/api/app/kv/my-key", json!({ "key": "my-key", "deleted": true })).await;

    let client = test_client(&server).await;
    let result = client.kv.set("my-key", json!("hello"), None).await.unwrap();
    assert_eq!(result["value"], json!("hello"));
    assert!(client.kv.delete("my-key").await.unwrap());
}

#[tokio::test]
async fn test_kv_delete_not_found_returns_false() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/app/kv/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "status": "fail", "message": "not found" })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    // JSend fail on 404 surfaces as Api error, not NotFound — delete() only maps NotFound.
    let result = client.kv.delete("missing").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_kv_exists_ttl_list() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/app/kv/k/exists", json!({ "key": "k", "exists": true })).await;
    mock_jsend(&server, "GET", "/api/app/kv/k/ttl", json!({ "key": "k", "ttl": 120 })).await;
    mock_jsend(&server, "GET", "/api/app/kv/", json!({ "keys": ["a", "b"] })).await;

    let client = test_client(&server).await;
    assert!(client.kv.exists("k").await.unwrap());
    assert_eq!(client.kv.ttl("k").await.unwrap(), Some(120));
    assert_eq!(client.kv.list_keys(None).await.unwrap(), vec!["a", "b"]);
}

#[tokio::test]
async fn test_kv_increment_decrement() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/app/kv/counter/increment"))
        .and(query_param("ttl", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jsend(json!({ "key": "counter", "value": 6 }))))
        .mount(&server)
        .await;
    mock_jsend(&server, "POST", "/api/app/kv/counter/decrement", json!({ "key": "counter", "value": 4 })).await;

    let client = test_client(&server).await;
    let inc = client.kv.increment("counter", Some(2), Some(60)).await.unwrap();
    assert_eq!(inc["value"], json!(6));
    let dec = client.kv.decrement("counter", None).await.unwrap();
    assert_eq!(dec["value"], json!(4));
}

#[tokio::test]
async fn test_kv_batch_ops() {
    let server = MockServer::start().await;
    mock_jsend(&server, "POST", "/api/app/kv/batch/get", json!({ "values": { "k1": "v1", "k2": null } })).await;
    mock_jsend(&server, "POST", "/api/app/kv/batch/set", json!({ "status": "ok" })).await;
    mock_jsend(&server, "POST", "/api/app/kv/batch/delete", json!({ "deleted": 2 })).await;

    let client = test_client(&server).await;
    let values = client.kv.batch_get(&["k1".into(), "k2".into()]).await.unwrap();
    assert_eq!(values.get("k1"), Some(&Some(json!("v1"))));
    let pairs = vec![KvPair { key: "k1".into(), value: json!("v1"), ttl: None }];
    client.kv.batch_set(&pairs).await.unwrap();
    assert_eq!(client.kv.batch_delete(&["k1".into(), "k2".into()]).await.unwrap(), 2);
}

// ─── db (proxy via install id) ──────────────────────────────────────────────

#[tokio::test]
async fn test_db_query_with_install_id() {
    let server = MockServer::start().await;
    mock_jsend(&server, "POST", "/p/7/db/query", json!({
        "columns": ["id", "name"],
        "rows": [[1, "Alice"]],
        "row_count": 1,
        "truncated": false,
        "execution_time_ms": 1.5
    })).await;

    let client = test_client(&server).await;
    let result = client.db.query_with_install_id(7, "SELECT * FROM items", None, None, None).await.unwrap();
    assert_eq!(result.columns, vec!["id", "name"]);
    assert_eq!(result.row_count, 1);
}

#[tokio::test]
async fn test_db_execute_with_base_path() {
    let server = MockServer::start().await;
    mock_jsend(&server, "POST", "/p/7/db/execute", json!({ "rows_affected": 1 })).await;

    let client = test_client(&server).await;
    let result = client.db.execute_with_base_path("/p/7", "UPDATE t SET x = 1", None).await.unwrap();
    assert_eq!(result["rows_affected"], json!(1));
}

// ─── settings / app_settings ────────────────────────────────────────────────

#[tokio::test]
async fn test_platform_settings() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/settings", json!({ "platform_name": "Alcedo" })).await;
    mock_jsend(&server, "PUT", "/api/platform/settings", json!({ "platform_name": "Alcedo2" })).await;

    let client = test_client(&server).await;
    assert_eq!(client.settings.get().await.unwrap().platform_name, "Alcedo");
    let updated = client.settings.update(json!({ "platform_name": "Alcedo2" })).await.unwrap();
    assert_eq!(updated.platform_name, "Alcedo2");
}

#[tokio::test]
async fn test_app_settings() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/app/settings", json!({ "site_name": "Demo" })).await;
    mock_jsend(&server, "PUT", "/api/app/settings/site_name", json!({ "key": "site_name", "updated_at": "2026-01-01", "value": "New" })).await;
    mock_jsend(&server, "POST", "/api/app/settings/batch", json!({ "count": 2, "success": true })).await;

    let client = test_client(&server).await;
    let all = client.app_settings.list().await.unwrap();
    assert_eq!(all.get("site_name"), Some(&json!("Demo")));
    let one = client.app_settings.update("site_name", json!("New")).await.unwrap();
    assert_eq!(one.key, "site_name");
    let batch = client.app_settings.batch(std::collections::HashMap::from([("a".to_string(), json!(1))])).await.unwrap();
    assert!(batch.success);
}

// ─── logs / activity_logs ───────────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_logs() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/plugins/test-plugin/logs", json!([{ "id": "r1" }])).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/test-plugin/logs/r1", json!({ "id": "r1", "detail": true })).await;

    let client = test_client(&server).await;
    let logs = client.logs.list(LogListParams::default()).await.unwrap();
    assert_eq!(logs[0]["id"], json!("r1"));
    let detail = client.logs.detail("test-plugin", "r1").await.unwrap();
    assert_eq!(detail["detail"], json!(true));
}

#[tokio::test]
async fn test_activity_logs() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/app/logs/collections", json!({ "data": [], "total": 0, "limit": 50, "offset": 0 })).await;
    mock_jsend(&server, "GET", "/api/app/logs/system", json!({ "data": [], "total": 0, "limit": 50, "offset": 0 })).await;
    mock_jsend(&server, "GET", "/api/platform/logs", json!({ "data": [], "total": 0, "limit": 50, "offset": 0 })).await;

    let client = test_client(&server).await;
    assert_eq!(client.activity_logs.list_collections(None).await.unwrap().total, 0);
    assert_eq!(client.activity_logs.list_app_system(None).await.unwrap().total, 0);
    assert_eq!(client.activity_logs.list_system(None).await.unwrap().total, 0);
}

// ─── migrations / schema ────────────────────────────────────────────────────

#[tokio::test]
async fn test_migrations() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/plugins/test-plugin/migrations", json!([{ "version": "001" }])).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/test-plugin/migrations", json!({ "applied": ["001"] })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/test-plugin/rollback/001", json!({ "rolled_back": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.migrations.list().await.unwrap()[0]["version"], json!("001"));
    client.migrations.run().await.unwrap();
    client.migrations.rollback("001").await.unwrap();
}

#[tokio::test]
async fn test_schema_get() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/plugins/test-plugin/schema", json!({
        "plugin_name": "test-plugin",
        "schema_name": "plugin_test_plugin",
        "tables": [{ "name": "items", "columns": [{ "name": "id", "data_type": "integer", "is_nullable": false, "is_primary_key": true }] }]
    })).await;

    let client = test_client(&server).await;
    let schema = client.schema.get().await.unwrap();
    assert_eq!(schema.plugin_name, "test-plugin");
    assert!(schema.tables[0].columns[0].is_primary_key);
}

// ─── auth / sessions / users ────────────────────────────────────────────────

#[tokio::test]
async fn test_auth() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/auth/me", json!({
        "user": { "id": "u1", "email": "a@b.c", "display_name": "A", "is_admin": true, "created_at": "", "updated_at": "" },
        "scopes": ["users.all"]
    })).await;
    mock_jsend(&server, "POST", "/api/platform/auth/login", json!({
        "user": { "id": "u1", "email": "a@b.c", "display_name": "A", "is_admin": true, "created_at": "", "updated_at": "" }
    })).await;
    mock_jsend(&server, "POST", "/api/platform/auth/logout", json!({ "success": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.auth.me().await.unwrap().user.id, "u1");
    assert_eq!(client.auth.login(json!({ "email": "a@b.c", "password": "x" })).await.unwrap().user.email, "a@b.c");
    assert_eq!(client.auth.logout().await.unwrap()["success"], json!(true));
}

#[tokio::test]
async fn test_sessions() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/sessions", json!([{ "id": "s1", "user_id": "u1", "current": true }])).await;
    mock_jsend(&server, "DELETE", "/api/platform/sessions/s1", json!(true)).await;
    mock_jsend(&server, "DELETE", "/api/platform/sessions", json!(true)).await;

    let client = test_client(&server).await;
    assert_eq!(client.sessions.list().await.unwrap()[0].id, "s1");
    assert!(client.sessions.revoke("s1").await.unwrap());
    assert!(client.sessions.revoke_all().await.unwrap());
}

#[tokio::test]
async fn test_users() {
    let server = MockServer::start().await;
    let user = json!({ "id": "u1", "email": "a@b.c", "display_name": "A", "is_admin": false, "created_at": "", "updated_at": "" });
    mock_jsend(&server, "GET", "/api/platform/users", json!([user])).await;
    mock_jsend(&server, "GET", "/api/platform/users/u1", user.clone()).await;
    mock_jsend(&server, "POST", "/api/platform/users", user.clone()).await;
    mock_jsend(&server, "PUT", "/api/platform/users/u1", user).await;
    mock_jsend(&server, "DELETE", "/api/platform/users/u1", json!({ "success": true })).await;
    mock_jsend(&server, "POST", "/api/platform/users/u1/password", json!({ "success": true })).await;
    mock_jsend(&server, "GET", "/api/platform/users/u1/sessions", json!([])).await;
    mock_jsend(&server, "DELETE", "/api/platform/users/u1/sessions/s1", json!({ "success": true })).await;
    mock_jsend(&server, "DELETE", "/api/platform/users/u1/sessions", json!({ "success": true })).await;
    mock_jsend(&server, "GET", "/api/app/users/u1/roles", json!([])).await;
    mock_jsend(&server, "POST", "/api/app/users/u1/roles", json!({ "ok": true })).await;
    mock_jsend(&server, "DELETE", "/api/app/users/u1/roles/r1", json!({ "ok": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.users.list().await.unwrap().len(), 1);
    assert_eq!(client.users.get("u1").await.unwrap().id, "u1");
    client.users.create(json!({ "email": "a@b.c" })).await.unwrap();
    client.users.update("u1", json!({ "display_name": "B" })).await.unwrap();
    client.users.delete("u1").await.unwrap();
    client.users.change_password("u1", json!({ "new_password": "x" })).await.unwrap();
    client.users.sessions("u1").await.unwrap();
    client.users.revoke_session("u1", "s1").await.unwrap();
    client.users.revoke_all_sessions("u1").await.unwrap();
    client.users.list_roles("u1").await.unwrap();
    client.users.assign_role("u1", "r1").await.unwrap();
    client.users.remove_role("u1", "r1").await.unwrap();
}

// ─── apps / versions ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_apps() {
    let server = MockServer::start().await;
    let app = json!({ "id": 1, "name": "App", "api_name": "app", "icon": null, "logo": null, "versions": ["production"] });
    mock_jsend(&server, "GET", "/api/platform/apps", json!([app])).await;
    mock_jsend(&server, "GET", "/api/platform/apps/1", app.clone()).await;
    mock_jsend(&server, "POST", "/api/platform/apps", app.clone()).await;
    mock_jsend(&server, "PUT", "/api/platform/apps/1", app).await;
    mock_jsend(&server, "DELETE", "/api/platform/apps/1", json!({ "success": true })).await;
    mock_jsend(&server, "GET", "/api/platform/me/apps", json!([{ "app_id": 1, "app_name": "App", "api_name": "app", "version": "production", "roles": [] }])).await;
    mock_jsend(&server, "GET", "/api/platform/app-versions", json!([{ "id": 5, "app_id": 1, "app_name": "App", "api_name": "app", "version_id": 2, "version_name": "production" }])).await;
    mock_jsend(&server, "GET", "/api/platform/users/u1/app-access", json!([])).await;
    mock_jsend(&server, "PUT", "/api/platform/users/u1/app-access", json!({ "success": true })).await;
    mock_jsend(&server, "GET", "/api/platform/apps/1/versions/2/access", json!([])).await;
    mock_jsend(&server, "PUT", "/api/platform/apps/1/versions/2/access", json!({ "success": true })).await;
    mock_jsend(&server, "DELETE", "/api/platform/apps/1/versions/2/access/u1", json!({ "success": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.apps.list().await.unwrap()[0].id, 1);
    assert_eq!(client.apps.get(1).await.unwrap().api_name, "app");
    client.apps.create(json!({ "name": "App", "api_name": "app", "version": "production" })).await.unwrap();
    client.apps.update(1, json!({ "name": "App2" })).await.unwrap();
    client.apps.delete(1).await.unwrap();
    assert_eq!(client.apps.me().await.unwrap()[0].app_id, 1);
    assert_eq!(client.apps.app_versions().await.unwrap()[0].id, 5);
    client.apps.get_user_access("u1").await.unwrap();
    client.apps.set_user_access("u1", json!({ "app": "app", "version": "production", "role_ids": [] })).await.unwrap();
    client.apps.get_app_version_access(1, 2).await.unwrap();
    client.apps.set_app_version_access(1, 2, json!({ "user_ids": [] })).await.unwrap();
    client.apps.revoke_app_version_access(1, 2, "u1").await.unwrap();
}

#[tokio::test]
async fn test_versions() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/versions", json!([{ "id": 2, "version_name": "production" }])).await;
    mock_jsend(&server, "POST", "/api/platform/versions", json!({ "id": 3, "version_name": "staging" })).await;
    mock_jsend(&server, "DELETE", "/api/platform/versions/3", json!({ "success": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.versions.list().await.unwrap()[0].version_name, "production");
    assert_eq!(client.versions.create(json!({ "version_name": "staging" })).await.unwrap().id, 3);
    client.versions.delete(3).await.unwrap();
}

// ─── roles / policies ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_roles() {
    let server = MockServer::start().await;
    for (m, p) in [
        ("GET", "/api/app/roles"),
        ("POST", "/api/app/roles"),
        ("GET", "/api/app/roles/r1"),
        ("PUT", "/api/app/roles/r1"),
        ("DELETE", "/api/app/roles/r1"),
        ("GET", "/api/app/roles/r1/permissions"),
        ("POST", "/api/app/roles/r1/permissions"),
        ("DELETE", "/api/app/roles/r1/permissions/p1"),
        ("GET", "/api/app/roles/r1/policies"),
        ("POST", "/api/app/roles/r1/policies"),
        ("DELETE", "/api/app/roles/r1/policies/pol1"),
    ] {
        mock_jsend(&server, m, p, json!({ "ok": true })).await;
    }

    let client = test_client(&server).await;
    client.roles.list().await.unwrap();
    client.roles.create(json!({ "name": "r" })).await.unwrap();
    client.roles.get("r1").await.unwrap();
    client.roles.update("r1", json!({ "name": "r2" })).await.unwrap();
    client.roles.delete("r1").await.unwrap();
    client.roles.list_permissions("r1").await.unwrap();
    client.roles.update_permissions("r1", json!({ "scopes": [] })).await.unwrap();
    client.roles.delete_permission("r1", "p1").await.unwrap();
    client.roles.list_policies("r1").await.unwrap();
    client.roles.assign_policy("r1", "pol1").await.unwrap();
    client.roles.remove_policy("r1", "pol1").await.unwrap();
}

#[tokio::test]
async fn test_policies() {
    let server = MockServer::start().await;
    for (m, p) in [
        ("GET", "/api/app/policies"),
        ("POST", "/api/app/policies"),
        ("GET", "/api/app/policies/pol1"),
        ("PUT", "/api/app/policies/pol1"),
        ("DELETE", "/api/app/policies/pol1"),
        ("GET", "/api/app/policies/pol1/permissions"),
        ("POST", "/api/app/policies/pol1/permissions"),
        ("PUT", "/api/app/policies/pol1/permissions/perm1"),
        ("DELETE", "/api/app/policies/pol1/permissions/perm1"),
        ("DELETE", "/api/app/policies/pol1/permissions/collection/orders"),
        ("GET", "/api/app/policies/pol1/plugins"),
        ("GET", "/api/app/plugins/test-plugin/policies"),
        ("POST", "/api/app/plugins/test-plugin/policies"),
        ("DELETE", "/api/app/plugins/test-plugin/policies/pol1"),
    ] {
        mock_jsend(&server, m, p, json!({ "ok": true })).await;
    }

    let client = test_client(&server).await;
    client.policies.list().await.unwrap();
    client.policies.create(json!({ "name": "p" })).await.unwrap();
    client.policies.get("pol1").await.unwrap();
    client.policies.update("pol1", json!({})).await.unwrap();
    client.policies.delete("pol1").await.unwrap();
    client.policies.list_permissions("pol1").await.unwrap();
    client.policies.create_permission("pol1", json!({})).await.unwrap();
    client.policies.update_permission("pol1", "perm1", json!({})).await.unwrap();
    client.policies.delete_permission("pol1", "perm1").await.unwrap();
    client.policies.delete_collection_permissions("pol1", "orders").await.unwrap();
    client.policies.list_assigned_plugins("pol1").await.unwrap();
    client.policies.list_plugin_policies("test-plugin").await.unwrap();
    client.policies.assign_plugin_policy("test-plugin", "pol1").await.unwrap();
    client.policies.unassign_plugin_policy("test-plugin", "pol1").await.unwrap();
}

// ─── collections ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_collections() {
    let server = MockServer::start().await;
    let collection = json!({
        "created_at": "2026-01-01", "name": "orders", "updated_at": "2026-01-01",
        "fields": [], "is_system": false
    });
    mock_jsend(&server, "GET", "/api/app/collections", json!({ "collections": [collection] })).await;
    mock_jsend(&server, "GET", "/api/app/collections/schema", json!({ "collections": [] })).await;
    mock_jsend(&server, "GET", "/api/app/collections/schema/ts", json!("export type Orders = {};")).await;
    mock_jsend(&server, "GET", "/api/app/collections/orders", collection.clone()).await;
    mock_jsend(&server, "POST", "/api/app/collections", collection.clone()).await;
    mock_jsend(&server, "PUT", "/api/app/collections/orders", json!({ "success": true })).await;
    mock_jsend(&server, "DELETE", "/api/app/collections/orders", json!({ "deleted": true })).await;
    mock_jsend(&server, "GET", "/api/app/collections/orders/$create", json!({ "create": true })).await;
    mock_jsend(&server, "GET", "/api/app/collections/orders/layouts", json!([])).await;
    mock_jsend(&server, "POST", "/api/app/collections/orders/layouts", json!({ "id": "l1" })).await;
    mock_jsend(&server, "PUT", "/api/app/collections/orders/layouts/l1", json!({ "ok": true })).await;
    mock_jsend(&server, "DELETE", "/api/app/collections/orders/layouts/l1", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/app/collections/orders/layouts/l1/roles", json!([])).await;
    mock_jsend(&server, "PUT", "/api/app/collections/orders/layouts/l1/roles", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/app/collections/orders/layout", json!({ "id": "l1" })).await;
    mock_jsend(&server, "GET", "/api/app/collections/orders/layouts/l1/sections", json!([])).await;
    mock_jsend(&server, "POST", "/api/app/collections/orders/layouts/l1/sections", json!({ "id": "s1" })).await;
    mock_jsend(&server, "PATCH", "/api/app/collections/orders/layouts/l1/sections", json!({ "ok": true })).await;
    mock_jsend(&server, "PUT", "/api/app/collections/orders/layouts/l1/sections/s1", json!({ "ok": true })).await;
    mock_jsend(&server, "DELETE", "/api/app/collections/orders/layouts/l1/sections/s1", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/app/collections/orders/views", json!([])).await;
    mock_jsend(&server, "POST", "/api/app/collections/orders/views", json!({ "id": "v1" })).await;
    mock_jsend(&server, "PUT", "/api/app/collections/orders/views/v1", json!({ "ok": true })).await;
    mock_jsend(&server, "DELETE", "/api/app/collections/orders/views/v1", json!({ "ok": true })).await;
    mock_jsend(&server, "PUT", "/api/app/collections/orders/views/v1/default", json!({ "ok": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.collections.list().await.unwrap().collections[0].name, "orders");
    client.collections.schema().await.unwrap();
    assert!(client.collections.schema_ts().await.unwrap().contains("Orders"));
    assert_eq!(client.collections.get("orders").await.unwrap().name, "orders");
    client.collections.create(json!({ "name": "orders" })).await.unwrap();
    client.collections.update("orders", json!({})).await.unwrap();
    client.collections.delete("orders").await.unwrap();
    assert_eq!(client.collections.get_create_policy("orders").await.unwrap()["create"], json!(true));
    client.collections.list_layouts("orders").await.unwrap();
    client.collections.create_layout("orders", json!({})).await.unwrap();
    client.collections.update_layout("orders", "l1", json!({})).await.unwrap();
    client.collections.delete_layout("orders", "l1").await.unwrap();
    client.collections.get_layout_roles("orders", "l1").await.unwrap();
    client.collections.set_layout_roles("orders", "l1", &["r1".to_string()]).await.unwrap();
    client.collections.get_resolved_layout("orders").await.unwrap();
    client.collections.list_layout_sections("orders", "l1").await.unwrap();
    client.collections.create_layout_section("orders", "l1", json!({})).await.unwrap();
    client.collections.batch_reorder_sections("orders", "l1", json!([])).await.unwrap();
    client.collections.update_layout_section("orders", "l1", "s1", json!({})).await.unwrap();
    client.collections.delete_layout_section("orders", "l1", "s1").await.unwrap();
    client.collections.list_views("orders").await.unwrap();
    client.collections.create_view("orders", json!({})).await.unwrap();
    client.collections.update_view("orders", "v1", json!({})).await.unwrap();
    client.collections.delete_view("orders", "v1").await.unwrap();
    client.collections.set_default_view("orders", "v1").await.unwrap();
}

// ─── items ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_items_crud() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/app/items/orders", json!({ "data": [{ "id": "1" }], "limit": 100, "offset": 0 })).await;
    mock_jsend(&server, "POST", "/api/app/items/orders", json!({ "id": "1" })).await;
    mock_jsend(&server, "PATCH", "/api/app/items/orders", json!({ "updated": 1 })).await;
    mock_jsend(&server, "DELETE", "/api/app/items/orders", json!({ "deleted": 1 })).await;
    mock_jsend(&server, "GET", "/api/app/items/orders/1", json!({ "id": "1" })).await;
    mock_jsend(&server, "PATCH", "/api/app/items/orders/1", json!({ "id": "1", "status": "done" })).await;
    mock_jsend(&server, "GET", "/api/app/items/orders/1/references", json!([])).await;
    mock_jsend(&server, "POST", "/api/app/items/orders/$delete", json!({ "1": true })).await;
    mock_jsend(&server, "POST", "/api/app/items/orders/$update", json!({ "1": true })).await;
    mock_jsend(&server, "POST", "/api/app/items/orders/$permissions", json!({ "create": true })).await;
    // "users" collection is served by the platform users endpoint
    mock_jsend(&server, "GET", "/api/platform/users", json!([])).await;

    let client = test_client(&server).await;
    let list = client.items.list("orders", None).await.unwrap();
    assert_eq!(list["data"][0]["id"], json!("1"));
    client.items.create("orders", json!({ "status": "new" })).await.unwrap();
    client.items.update("orders", json!({ "id": "1" }), json!({ "status": "done" })).await.unwrap();
    client.items.delete("orders", json!({ "pk_values": ["1"] })).await.unwrap();
    assert_eq!(client.items.get("orders", "1", None).await.unwrap()["id"], json!("1"));
    client.items.patch("orders", "1", json!({ "status": "done" })).await.unwrap();
    client.items.references("orders", "1").await.unwrap();
    assert_eq!(client.items.delete_permissions("orders", &[json!("1")]).await.unwrap().get("1"), Some(&true));
    assert_eq!(client.items.update_permissions("orders", &[json!("1")]).await.unwrap().get("1"), Some(&true));
    assert_eq!(client.items.permissions("orders", "create", json!({}), None).await.unwrap()["create"], json!(true));
    client.items.list("users", None).await.unwrap();
}

#[tokio::test]
async fn test_items_query_group_by_aggregate_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/app/items/orders"))
        .and(query_param("groupBy[]", "status"))
        .and(query_param("groupBy[]", "region"))
        .and(query_param("aggregate[count]", "id"))
        .and(query_param("aggregate[sum]", "total"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jsend(json!({ "data": [] }))))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let query = ItemQuery {
        limit: Some(10),
        group_by: vec!["status".to_string(), "region".to_string()],
        aggregate: vec![
            ("count".to_string(), "id".to_string()),
            ("sum".to_string(), "total".to_string()),
        ],
        ..Default::default()
    };
    client.items.query("orders", &query).await.unwrap();
}

// ─── lookup / menus ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_lookup_users() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/app/lookup/users", json!([{ "id": "u1", "email": "a@b.c" }])).await;

    let client = test_client(&server).await;
    let users = client.lookup.users().await.unwrap();
    assert_eq!(users[0].id, "u1");
}

#[tokio::test]
async fn test_menus() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/app/menus", json!([{ "id": "m1", "name": "Main", "icon": "", "role_count": 1, "item_count": 2, "created_at": "" }])).await;
    mock_jsend(&server, "GET", "/api/app/menus/m1", json!({ "id": "m1", "name": "Main", "icon": "", "sections": [] })).await;
    mock_jsend(&server, "POST", "/api/app/menus", json!({ "id": "m1" })).await;
    mock_jsend(&server, "PUT", "/api/app/menus/m1", json!({ "ok": true })).await;
    mock_jsend(&server, "DELETE", "/api/app/menus/m1", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/app/menus/my", json!([])).await;
    mock_jsend(&server, "GET", "/api/app/menus/m1/roles", json!([])).await;
    mock_jsend(&server, "PUT", "/api/app/menus/m1/roles", json!({ "ok": true })).await;
    mock_jsend(&server, "POST", "/api/app/menus/m1/copy", json!({ "ok": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.menus.list().await.unwrap()[0].id, "m1");
    assert_eq!(client.menus.get("m1").await.unwrap().name, "Main");
    client.menus.create(json!({ "name": "Main" })).await.unwrap();
    client.menus.update("m1", json!({})).await.unwrap();
    client.menus.delete("m1").await.unwrap();
    client.menus.my().await.unwrap();
    client.menus.get_roles("m1").await.unwrap();
    client.menus.set_roles("m1", &["r1".to_string()]).await.unwrap();
    client.menus.copy("m1", "m2").await.unwrap();
}

// ─── files ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_files() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/app/files", json!({ "data": [{ "id": "f1", "filename": "a.png" }], "total": 1 })).await;
    mock_jsend(&server, "GET", "/api/app/files/f1", json!({ "id": "f1", "filename": "a.png" })).await;
    mock_jsend(&server, "PATCH", "/api/app/files/f1", json!({ "ok": true })).await;
    mock_jsend(&server, "DELETE", "/api/app/files/f1", json!({ "ok": true })).await;
    mock_jsend(&server, "POST", "/api/app/files/batch/delete", json!({ "deleted": 1 })).await;
    mock_jsend(&server, "POST", "/api/app/files/upload", json!({ "id": "f2" })).await;
    mock_jsend(&server, "GET", "/api/app/files/folders", json!([])).await;
    mock_jsend(&server, "POST", "/api/app/files/folders", json!({ "id": "fo1" })).await;
    mock_jsend(&server, "GET", "/api/app/files/folders/fo1", json!({ "id": "fo1" })).await;
    mock_jsend(&server, "PATCH", "/api/app/files/folders/fo1", json!({ "ok": true })).await;
    mock_jsend(&server, "DELETE", "/api/app/files/folders/fo1", json!({ "ok": true })).await;
    Mock::given(method("GET"))
        .and(path("/api/app/files/f1/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"binary-data".to_vec()))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    assert_eq!(client.files.list(None).await.unwrap().total, 1);
    assert_eq!(client.files.get("f1").await.unwrap().filename, Some("a.png".to_string()));
    client.files.update("f1", json!({ "alt_text": "x" })).await.unwrap();
    client.files.delete("f1").await.unwrap();
    client.files.batch_delete(&["f1".to_string()]).await.unwrap();
    let uploaded = client.files.upload(b"hello".to_vec(), "a.txt", None).await.unwrap();
    assert_eq!(uploaded["id"], json!("f2"));
    client.files.list_folders(None).await.unwrap();
    client.files.create_folder("docs", None).await.unwrap();
    client.files.get_folder("fo1").await.unwrap();
    client.files.update_folder("fo1", json!({ "name": "docs2" })).await.unwrap();
    client.files.delete_folder("fo1", true).await.unwrap();
    let bytes = client.files.download("f1").await.unwrap();
    assert_eq!(bytes, b"binary-data".to_vec());
    assert!(client.files.download_url("f1").ends_with("/api/app/files/f1/download"));
}

// ─── registries ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_registries() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/registries", json!([{ "id": 1 }])).await;
    mock_jsend(&server, "GET", "/api/platform/registries/1", json!({ "id": 1, "url": "localhost:5000" })).await;
    mock_jsend(&server, "POST", "/api/platform/registries", json!({ "id": 2 })).await;
    mock_jsend(&server, "PUT", "/api/platform/registries/1", json!({ "id": 1 })).await;
    mock_jsend(&server, "DELETE", "/api/platform/registries/1", json!({ "success": true })).await;
    mock_jsend(&server, "POST", "/api/platform/registries/health-check", json!({ "healthy": true })).await;
    mock_jsend(&server, "GET", "/api/platform/registries/1/health", json!({ "healthy": true })).await;
    mock_jsend(&server, "GET", "/api/platform/registries/1/images", json!([])).await;

    let client = test_client(&server).await;
    assert_eq!(client.registries.list().await.unwrap()[0]["id"], json!(1));
    client.registries.get(1).await.unwrap();
    client.registries.create(json!({ "url": "x" })).await.unwrap();
    client.registries.update(1, json!({})).await.unwrap();
    client.registries.delete(1).await.unwrap();
    client.registries.health_check_url("localhost:5000").await.unwrap();
    client.registries.health_check(1).await.unwrap();
    client.registries.images(1).await.unwrap();
}

// ─── plugins ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_plugins_crud_and_deploy() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/plugins", json!([])).await;
    mock_jsend(&server, "POST", "/api/platform/plugins", json!({ "slug": "p" })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p", json!({ "slug": "p" })).await;
    mock_jsend(&server, "PUT", "/api/platform/plugins/p", json!({ "slug": "p" })).await;
    mock_jsend(&server, "DELETE", "/api/platform/plugins/p", json!({ "success": true })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/deploy", json!({ "install_id": 7 })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/preview", json!({ "manifest": {} })).await;

    let client = test_client(&server).await;
    client.plugins.list().await.unwrap();
    client.plugins.create(json!({ "slug": "p" })).await.unwrap();
    assert_eq!(client.plugins.get("p").await.unwrap()["slug"], json!("p"));
    client.plugins.update("p", json!({})).await.unwrap();
    client.plugins.delete("p").await.unwrap();
    assert_eq!(client.plugins.deploy(json!({ "slug": "p", "version": "1.0.0", "image": "img", "registry_id": 1 })).await.unwrap()["install_id"], json!(7));
    client.plugins.preview("img", Some(1)).await.unwrap();
}

#[tokio::test]
async fn test_plugin_installs_by_slug_and_app_version() {
    let server = MockServer::start().await;
    mock_jsend(&server, "DELETE", "/api/platform/plugins/p/installs/5", json!({ "success": true })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/p/installs/5/enable", json!({ "ok": true })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/p/installs/5/disable", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/installs/5/settings", json!({ "settings": {} })).await;
    mock_jsend(&server, "PATCH", "/api/platform/plugins/p/installs/5/settings", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/installs/5/settings/system", json!({ "settings": {} })).await;
    mock_jsend(&server, "PATCH", "/api/platform/plugins/p/installs/5/settings/system", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/installs/5/scopes", json!({ "scopes": [] })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/p/installs/5/scopes", json!({ "ok": true })).await;

    let client = test_client(&server).await;
    client.plugins.uninstall("p", 5).await.unwrap();
    client.plugins.enable("p", 5).await.unwrap();
    client.plugins.disable("p", 5).await.unwrap();
    client.plugins.get_settings("p", 5).await.unwrap();
    client.plugins.update_settings("p", 5, json!({})).await.unwrap();
    client.plugins.get_system_settings("p", 5).await.unwrap();
    client.plugins.update_system_settings("p", 5, json!({})).await.unwrap();
    client.plugins.get_scopes("p", 5).await.unwrap();
    client.plugins.update_scopes("p", 5, &["kv.all".to_string()]).await.unwrap();
}

#[tokio::test]
async fn test_plugin_installs_by_install_id() {
    let server = MockServer::start().await;
    mock_jsend(&server, "DELETE", "/api/platform/plugins/installs/7", json!({ "success": true })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/installs/7/enable", json!({ "ok": true })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/installs/7/disable", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/installs/7/settings", json!({ "settings": {} })).await;
    mock_jsend(&server, "PATCH", "/api/platform/plugins/installs/7/settings", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/installs/7/scopes", json!({ "scopes": [] })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/installs/7/scopes", json!({ "ok": true })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/installs/7/runtime", json!({ "status": "running" })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/installs/7/instances", json!([])).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/installs/7/instances/t1", json!({ "id": "t1" })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/installs/7/instances/t1/stats", json!({})).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/installs/7/instances/t1/logs", json!("log")).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/installs/7/versions", json!([])).await;

    let client = test_client(&server).await;
    client.plugins.uninstall_by_id(7).await.unwrap();
    client.plugins.enable_by_id(7).await.unwrap();
    client.plugins.disable_by_id(7).await.unwrap();
    client.plugins.get_settings_by_id(7).await.unwrap();
    client.plugins.update_settings_by_id(7, json!({})).await.unwrap();
    client.plugins.get_scopes_by_id(7).await.unwrap();
    client.plugins.update_scopes_by_id(7, &["kv.all".to_string()]).await.unwrap();
    assert_eq!(client.plugins.runtime_info(7).await.unwrap()["status"], json!("running"));
    client.plugins.instances(7).await.unwrap();
    client.plugins.instance_detail(7, "t1").await.unwrap();
    client.plugins.instance_stats(7, "t1").await.unwrap();
    client.plugins.instance_logs(7, "t1").await.unwrap();
    client.plugins.install_versions(7).await.unwrap();
}

#[tokio::test]
async fn test_plugin_info_and_lifecycle() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/versions", json!([])).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/logs", json!([])).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/logs/r1", json!({})).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/docs", json!([])).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/docs/README.md", json!("# hi")).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/schema", json!({})).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/events", json!([])).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/pages", json!({ "pages": [] })).await;
    mock_jsend(&server, "GET", "/api/platform/plugins/p/pages/assets", json!({ "assets": [] })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/p/scale", json!({ "ok": true })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/p/restart", json!({ "ok": true })).await;
    mock_jsend(&server, "POST", "/api/platform/plugins/p/stop", json!({ "ok": true })).await;

    let client = test_client(&server).await;
    client.plugins.versions("p").await.unwrap();
    client.plugins.request_logs("p", LogListParams::default()).await.unwrap();
    client.plugins.request_log_detail("p", "r1").await.unwrap();
    client.plugins.docs("p").await.unwrap();
    assert_eq!(client.plugins.doc_content("p", "docs/README.md").await.unwrap(), "# hi");
    client.plugins.schema("p").await.unwrap();
    client.plugins.events("p").await.unwrap();
    client.plugins.pages("p").await.unwrap();
    client.plugins.assets("p").await.unwrap();
    client.plugins.scale("p", json!({ "replicas": 2 })).await.unwrap();
    client.plugins.restart("p", None).await.unwrap();
    client.plugins.stop("p").await.unwrap();
}

// ─── route_mappings / developer_keys / dev ──────────────────────────────────

#[tokio::test]
async fn test_route_mappings() {
    let server = MockServer::start().await;
    let mapping = json!({ "id": 1, "domain": null, "path_prefix": "/api/x", "install_id": 7, "is_active": true });
    mock_jsend(&server, "GET", "/api/platform/settings/route-mappings", json!([mapping])).await;
    mock_jsend(&server, "POST", "/api/platform/settings/route-mappings", mapping.clone()).await;
    mock_jsend(&server, "PUT", "/api/platform/settings/route-mappings/1", mapping).await;
    mock_jsend(&server, "DELETE", "/api/platform/settings/route-mappings/1", json!({ "success": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.route_mappings.list().await.unwrap()[0].install_id, 7);
    client.route_mappings.create(json!({ "path_prefix": "/api/x", "install_id": 7 })).await.unwrap();
    client.route_mappings.update(1, json!({ "is_active": false })).await.unwrap();
    client.route_mappings.delete(1).await.unwrap();
}

#[tokio::test]
async fn test_developer_keys() {
    let server = MockServer::start().await;
    mock_jsend(&server, "GET", "/api/platform/developer-keys", json!([{ "id": "k1", "name": "ci" }])).await;
    mock_jsend(&server, "POST", "/api/platform/developer-keys", json!({ "id": "k2", "name": "ci2", "raw_key": "secret" })).await;
    mock_jsend(&server, "DELETE", "/api/platform/developer-keys/k2", json!({ "success": true })).await;

    let client = test_client(&server).await;
    assert_eq!(client.developer_keys.list(None).await.unwrap()[0].id, "k1");
    let created = client.developer_keys.create(2, "ci2").await.unwrap();
    assert_eq!(created.raw_key, "secret");
    client.developer_keys.delete("k2").await.unwrap();
}
