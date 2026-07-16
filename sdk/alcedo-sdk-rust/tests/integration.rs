use alcedo_sdk::*;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};
use serde_json::json;

/// Helper to create a client pointed at a wiremock server
async fn test_client(server: &MockServer) -> AlcedoClient {
    AlcedoClientBuilder::new()
        .base_url(server.uri())
        .plugin_slug("test-plugin")
        .build()
        .expect("Failed to build test client")
}

#[tokio::test]
async fn test_health_check() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok",
            "core": {
                "db": "connected",
                "docker": "swarm"
            },
            "plugins": []
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let health = client.health.check().await.unwrap();

    assert_eq!(health.status, "ok");
    assert_eq!(health.core.db, "connected");
    assert_eq!(health.core.docker, "swarm");
}

#[tokio::test]
async fn test_kv_get() {
    let server = MockServer::start().await;

    // KV get — key exists
    Mock::given(method("GET"))
        .and(path("/api/kv/my-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": "my-value"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let value = client.kv.get("my-key").await.unwrap();
    assert_eq!(value, Some(json!("my-value")));
}

#[tokio::test]
async fn test_kv_get_not_found() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/kv/missing-key"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.kv.get("missing-key").await;
    assert!(result.is_err());
    match result {
        Err(AlcedoError::NotFound { .. }) => {} // expected
        _ => panic!("Expected NotFound error, got {:?}", result),
    }
}

#[tokio::test]
async fn test_kv_set() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/api/kv/my-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": "stored"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.kv.set("my-key", json!("hello"), None).await.unwrap();
    assert_eq!(result, json!("stored"));
}

#[tokio::test]
async fn test_kv_set_with_ttl() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/api/kv/ttl-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": "stored"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.kv.set("ttl-key", json!("expires"), Some(60)).await.unwrap();
    assert_eq!(result, json!("stored"));
}

#[tokio::test]
async fn test_kv_delete() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/kv/delete-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let deleted = client.kv.delete("delete-key").await.unwrap();
    assert!(deleted);
}

#[tokio::test]
async fn test_kv_delete_not_found() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/kv/missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let deleted = client.kv.delete("missing").await.unwrap();
    assert!(!deleted); // 404 returns false, not error
}

#[tokio::test]
async fn test_kv_exists() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/kv/my-key/exists"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "exists": true
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let exists = client.kv.exists("my-key").await.unwrap();
    assert!(exists);
}

#[tokio::test]
async fn test_kv_ttl() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/kv/my-key/ttl"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ttl": 120
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let ttl = client.kv.ttl("my-key").await.unwrap();
    assert_eq!(ttl, Some(120));
}

#[tokio::test]
async fn test_kv_list_keys() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/kv/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keys": ["alpha", "beta", "gamma"]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let keys = client.kv.list_keys(None).await.unwrap();
    assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
}

#[tokio::test]
async fn test_kv_batch_get() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/kv/batch/get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": {
                "key1": "value1",
                "key2": null
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let values = client.kv.batch_get(&["key1".into(), "key2".into()]).await.unwrap();
    assert_eq!(values.get("key1"), Some(&Some(json!("value1"))));
    assert_eq!(values.get("key2"), Some(&None));
}

#[tokio::test]
async fn test_kv_batch_set() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/kv/batch/set"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let pairs = vec![KvPair {
        key: "k1".into(),
        value: json!("v1"),
        ttl: None,
    }];
    client.kv.batch_set(&pairs).await.unwrap();
}

#[tokio::test]
async fn test_kv_batch_delete() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/kv/batch/delete"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "deleted": 2
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let count = client.kv.batch_delete(&["k1".into(), "k2".into()]).await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_db_query() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/p/test-plugin/db/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "columns": ["id", "name"],
            "rows": [[1, "Alice"], [2, "Bob"]],
            "row_count": 2,
            "truncated": false,
            "execution_time_ms": 5.2
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.db.query("SELECT * FROM items", None, None, None).await.unwrap();
    assert_eq!(result.columns, vec!["id", "name"]);
    assert_eq!(result.row_count, 2);
    assert!(!result.truncated);
}

#[tokio::test]
async fn test_settings_get() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/plugins/test-plugin/settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "settings": { "theme": "dark" },
            "schema": {}
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let settings = client.settings.get().await.unwrap();
    assert_eq!(settings.settings.get("theme"), Some(&json!("dark")));
}

#[tokio::test]
async fn test_settings_update() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/api/plugins/test-plugin/settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "settings": { "theme": "light" }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.settings.update(json!({"theme": "light"})).await.unwrap();
    assert_eq!(result.get("settings").and_then(|s| s.get("theme")), Some(&json!("light")));
}

#[tokio::test]
async fn test_migrations_list() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/plugins/test-plugin/migrations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "version": "001",
                "name": "initial",
                "status": "applied",
                "applied_at": "2026-01-01T00:00:00Z"
            }
        ])))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let migrations = client.migrations.list().await.unwrap();
    assert_eq!(migrations.len(), 1);
    assert_eq!(migrations[0].version, "001");
    assert_eq!(migrations[0].status, "applied");
}

#[tokio::test]
async fn test_migrations_run() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/plugins/test-plugin/migrations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "applied": ["001_initial"],
            "errors": []
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    client.migrations.run().await.unwrap();
}

#[tokio::test]
async fn test_schema_get() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/plugins/test-plugin/schema"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plugin_name": "test-plugin",
            "schema_name": "plugin_test_plugin",
            "tables": [{
                "name": "items",
                "columns": [{
                    "name": "id",
                    "data_type": "integer",
                    "is_nullable": false,
                    "is_primary_key": true
                }]
            }]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let schema = client.schema.get().await.unwrap();
    assert_eq!(schema.plugin_name, "test-plugin");
    assert_eq!(schema.tables.len(), 1);
    assert!(schema.tables[0].columns[0].is_primary_key);
}

#[tokio::test]
async fn test_logs_list() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/plugins/test-plugin/logs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "log-1",
                "action": "query",
                "target": "db",
                "created_at": "2026-01-01T00:00:00Z"
            }
        ])))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let logs = client.logs.list(LogListParams::default()).await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].action, "query");
}

#[tokio::test]
async fn test_dev_start() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/dev/start"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "slug": "test-plugin",
            "url": "http://localhost:3000",
            "expires_at": "2026-01-01T01:00:00Z",
            "status": "active"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.dev.start("http://localhost:3000", None).await.unwrap();
    assert_eq!(result.get("status"), Some(&json!("active")));
}

#[tokio::test]
async fn test_dev_start_invalid_url() {
    let client = test_client(&MockServer::start().await).await;
    let result = client.dev.start("ftp://bad", None).await;
    assert!(result.is_err());
    match result {
        Err(AlcedoError::Validation { .. }) => {} // expected
        _ => panic!("Expected Validation error for invalid URL, got {:?}", result),
    }
}

#[tokio::test]
async fn test_dev_stop() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/dev/stop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "stopped"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    client.dev.stop().await.unwrap();
}

#[tokio::test]
async fn test_error_authentication() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/kv/secret"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.kv.get("secret").await;
    assert!(result.is_err());
    match result {
        Err(AlcedoError::Authentication { .. }) => {} // expected
        _ => panic!("Expected Authentication error, got {:?}", result),
    }
}

#[tokio::test]
async fn test_error_validation() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/api/kv/bad"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.kv.set("bad", json!("value"), None).await;
    assert!(result.is_err());
    match result {
        Err(AlcedoError::Validation { .. }) => {} // expected
        _ => panic!("Expected Validation error, got {:?}", result),
    }
}

#[tokio::test]
async fn test_error_server() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/kv/boom"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    let result = client.kv.get("boom").await;
    assert!(result.is_err());
    match result {
        Err(AlcedoError::Server { .. }) => {} // expected
        _ => panic!("Expected Server error, got {:?}", result),
    }
}

#[tokio::test]
async fn test_request_id_header() {
    use wiremock::matchers::header_exists;

    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .and(header_exists("X-Request-ID"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok",
            "core": { "db": "ok", "docker": "ok" },
            "plugins": []
        })))
        .mount(&server)
        .await;

    let client = test_client(&server).await;
    client.health.check().await.unwrap();
}
