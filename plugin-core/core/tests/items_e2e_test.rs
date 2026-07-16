use plugin_core::db::items::{create_items, delete_items, query_items, update_items, CreateRequest, DeleteRequest, UpdateRequest};
use plugin_core::db::query_builder::{QueryRequest, SortField};
use sqlx::PgPool;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use serde_json::json;

#[path = "common/mod.rs"]
mod common;

struct TestDb {
    pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestDb {
    async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (pool, container) = common::start_postgres().await?;
        Ok(Self { pool, _container: container })
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

async fn setup_test_table(pool: &PgPool, slug: &str) -> String {
    let schema = format!("plugin_{}", slug);
    sqlx::query(&format!(r#"CREATE SCHEMA IF NOT EXISTS "{}""#, schema))
        .execute(pool).await.unwrap();

    sqlx::query(&format!(
        r#"CREATE TABLE "{}"."items" (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            price NUMERIC(10,2),
            active BOOLEAN DEFAULT true,
            created_at TIMESTAMPTZ DEFAULT NOW()
        )"#, schema
    )).execute(pool).await.unwrap();

    schema
}

#[tokio::test]
async fn test_create_and_query_items() {
    let test_db = TestDb::new().await.unwrap();
    let schema = setup_test_table(test_db.pool(), "test_create_query").await;

    let items = vec![
        json!({"name": "Widget A", "price": 10.99, "active": true}).as_object().unwrap().clone(),
        json!({"name": "Widget B", "price": 20.50, "active": true}).as_object().unwrap().clone(),
        json!({"name": "Widget C", "price": 5.99, "active": false}).as_object().unwrap().clone(),
    ];

    let create_req = CreateRequest { items };
    let created = create_items(test_db.pool(), "test_create_query", create_req).await.unwrap();
    assert_eq!(created.len(), 3, "Should create 3 items");

    let query_req = QueryRequest {
        select: None,
        filters: None,
        sort: Some(vec![SortField { field: "price".to_string(), direction: "desc".to_string() }]),
        limit: 100,
        offset: 0,
        extra_select: None,
        extra_select_binds: None,
    };

    let response = query_items(test_db.pool(), "test_create_query", query_req).await.unwrap();
    assert!(!response.rows.is_empty(), "Should return items");
}

#[tokio::test]
async fn test_update_items() {
    let test_db = TestDb::new().await.unwrap();
    let schema = setup_test_table(test_db.pool(), "test_update").await;

    let items = vec![
        json!({"name": "Old Name", "price": 100.0}).as_object().unwrap().clone(),
    ];
    let create_req = CreateRequest { items };
    let _created = create_items(test_db.pool(), "test_update", create_req).await.unwrap();

    let update_req = UpdateRequest {
        filter: json!({"name": "Old Name"}),
        update: json!({"name": "New Name"}).as_object().unwrap().clone(),
    };
    let updated = update_items(test_db.pool(), "test_update", update_req).await.unwrap();
    assert_eq!(updated, 1, "Should update 1 item");
}

#[tokio::test]
async fn test_delete_items_by_pk() {
    let test_db = TestDb::new().await.unwrap();
    let schema = setup_test_table(test_db.pool(), "test_delete").await;

    let items = vec![
        json!({"name": "To Delete", "price": 50.0}).as_object().unwrap().clone(),
    ];
    let create_req = CreateRequest { items };
    let created = create_items(test_db.pool(), "test_delete", create_req).await.unwrap();
    let first_id = created[0].get("id").and_then(|v| v.as_i64()).unwrap();

    let delete_req = DeleteRequest {
        filter: None,
        pk_values: Some(vec![json!(first_id)]),
    };
    let (deleted_count, _) = delete_items(test_db.pool(), "test_delete", delete_req).await.unwrap();
    assert_eq!(deleted_count, 1, "Should delete 1 item");
}

#[tokio::test]
async fn test_invalid_column_rejected() {
    let test_db = TestDb::new().await.unwrap();
    let schema = setup_test_table(test_db.pool(), "test_invalid").await;

    let items = vec![
        json!({"nonexistent_column": "value"}).as_object().unwrap().clone(),
    ];
    let create_req = CreateRequest { items };
    let result = create_items(test_db.pool(), "test_invalid", create_req).await;
    assert!(result.is_err(), "Should reject invalid column");
}

#[tokio::test]
async fn test_non_existent_slug_returns_404() {
    let test_db = TestDb::new().await.unwrap();
    let result = query_items(
        test_db.pool(),
        "nonexistent",
        QueryRequest {
            select: None,
            filters: None,
            sort: None,
            limit: 10,
            offset: 0,
            extra_select: None,
            extra_select_binds: None,
        },
    ).await;
    assert!(result.is_err(), "Non-existent slug should error");
}
