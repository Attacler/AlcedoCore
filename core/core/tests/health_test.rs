#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

#[tokio::test]
async fn test_health_endpoint_with_db() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;
    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/health").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Health check failed: {}", response.text());

    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let core = body.get("core").expect("core field missing");
    let db_status = core.get("db").expect("db field missing");
    assert_eq!(db_status, "reachable", "DB should be reachable");
}

#[tokio::test]
async fn test_health_no_db() {
    let state = create_test_state_no_db().await;
    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/health").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let core = body.get("core").expect("core field missing");
    let db_status = core.get("db").expect("db field missing");
    assert_eq!(db_status, "no pool configured", "DB should show no pool configured");
}

#[tokio::test]
async fn test_test_endpoint() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;
    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/test").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK);
    assert_eq!(response.text(), "test ok");
}

#[tokio::test]
async fn test_test_state_endpoint() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;
    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/test-state").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK);
    assert!(response.text().contains("dev_mode=true"));
}
