#[path = "common/mod.rs"]
mod common;

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_db::core_state_for_migrations;
use alcedo_db::db::collection_items::{CreateItemsBody, UpdateItemsBody};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::tables::TableService;
use plugin_core::config::AppConfig;
use serde_json::json;

#[tokio::test]
async fn create_returns_inserted_row() {
    let (pool, _container) = common::start_postgres().await.unwrap();

    sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "default010v1""#).execute(&pool).await.unwrap();
    sqlx::query(r#"CREATE TABLE "default010v1"."notes" ("id" uuid PRIMARY KEY DEFAULT gen_random_uuid(), "title" text, "created_at" timestamptz, "updated_at" timestamptz)"#)
        .execute(&pool).await.unwrap();
    common::create_meta_tables(&pool).await;
    sqlx::query(r#"INSERT INTO "default010v1"."alcedocore_collection_definitions" (name) VALUES ('notes')"#)
        .execute(&pool).await.unwrap();
    common::add_field(&pool, "notes", "id", "uuid", 1, None, None).await;
    common::add_field(&pool, "notes", "title", "text", 2, None, None).await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext { app_name: "default".into(), version: "v1".into(), request_source: RequestSource::FirstMigration };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "notes".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);
    let mut map = serde_json::Map::new();
    map.insert("title".into(), json!("hello"));
    let outcome = service.create(&pool, CreateItemsBody::Single(map)).await.unwrap();

    assert_eq!(outcome.affected.len(), 1, "expected one created row");
    assert_eq!(outcome.affected[0].get("title"), Some(&json!("hello")));
    assert!(outcome.affected[0].get("id").is_some(), "row should have a generated id");
}

#[tokio::test]
async fn update_one_returns_new_row_and_old_pair() {
    let (pool, _container) = common::start_postgres().await.unwrap();

    sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "default010v1""#).execute(&pool).await.unwrap();
    sqlx::query(r#"CREATE TABLE "default010v1"."notes" ("id" uuid PRIMARY KEY DEFAULT gen_random_uuid(), "title" text, "created_at" timestamptz, "updated_at" timestamptz)"#)
        .execute(&pool).await.unwrap();
    common::create_meta_tables(&pool).await;
    sqlx::query(r#"INSERT INTO "default010v1"."alcedocore_collection_definitions" (name) VALUES ('notes')"#)
        .execute(&pool).await.unwrap();
    common::add_field(&pool, "notes", "id", "uuid", 1, None, None).await;
    common::add_field(&pool, "notes", "title", "text", 2, None, None).await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext { app_name: "default".into(), version: "v1".into(), request_source: RequestSource::FirstMigration };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "notes".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let mut create_map = serde_json::Map::new();
    create_map.insert("title".into(), json!("hello"));
    let created = service.create(&pool, CreateItemsBody::Single(create_map)).await.unwrap();
    let id = created.affected[0]
        .get("id")
        .and_then(|v| v.as_str())
        .expect("created row should have an id")
        .to_string();

    let mut update_map = serde_json::Map::new();
    update_map.insert("title".into(), json!("world"));
    let outcome = service.update_one(&pool, &id, &update_map, &[]).await.unwrap();

    assert_eq!(outcome.affected.len(), 1, "expected one updated row");
    assert_eq!(outcome.affected[0].get("title"), Some(&json!("world")));
    assert_eq!(outcome.pairs.len(), 1, "expected one old/new pair");
    assert_eq!(outcome.pairs[0].0.get("title"), Some(&json!("hello")));
    assert_eq!(outcome.pairs[0].1.get("title"), Some(&json!("world")));
}

#[tokio::test]
async fn bulk_update_returns_affected_rows() {
    let (pool, _container) = common::start_postgres().await.unwrap();

    sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "default010v1""#).execute(&pool).await.unwrap();
    sqlx::query(r#"CREATE TABLE "default010v1"."notes" ("id" uuid PRIMARY KEY DEFAULT gen_random_uuid(), "title" text, "created_at" timestamptz, "updated_at" timestamptz)"#)
        .execute(&pool).await.unwrap();
    common::create_meta_tables(&pool).await;
    sqlx::query(r#"INSERT INTO "default010v1"."alcedocore_collection_definitions" (name) VALUES ('notes')"#)
        .execute(&pool).await.unwrap();
    common::add_field(&pool, "notes", "id", "uuid", 1, None, None).await;
    common::add_field(&pool, "notes", "title", "text", 2, None, None).await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext { app_name: "default".into(), version: "v1".into(), request_source: RequestSource::FirstMigration };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "notes".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let mut first = serde_json::Map::new();
    first.insert("title".into(), json!("first"));
    let mut second = serde_json::Map::new();
    second.insert("title".into(), json!("second"));
    service
        .create(&pool, CreateItemsBody::Multiple(vec![first, second]))
        .await
        .unwrap();

    let mut update = serde_json::Map::new();
    update.insert("title".into(), json!("updated"));
    let body = UpdateItemsBody {
        filter: json!({ "title": "first" }),
        update,
    };
    let outcome = service.update(&pool, body, None).await.unwrap();

    assert_eq!(outcome.affected_count, 1, "expected exactly one updated row");
    assert_eq!(outcome.affected.len(), 1, "expected one updated row returned");
    assert_eq!(outcome.affected[0].get("title"), Some(&json!("updated")));
}
