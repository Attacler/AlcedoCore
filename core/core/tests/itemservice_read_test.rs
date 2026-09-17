#[path = "common/mod.rs"]
mod common;

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_common::error::AppError;
use alcedo_db::core_state_for_migrations;
use alcedo_db::db::filter_condition::SortField;
use alcedo_db::services::items::read::{ListRequest, OneRequest};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::tables::TableService;
use plugin_core::config::AppConfig;
use serde_json::json;

async fn make_collections(pool: &sqlx::PgPool) {
    sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "default010v1""#).execute(pool).await.unwrap();
    sqlx::query(r#"CREATE TABLE "default010v1"."authors" ("id" uuid PRIMARY KEY, "name" text, "country" text, "created_at" timestamptz, "updated_at" timestamptz)"#)
        .execute(pool).await.unwrap();
    sqlx::query(r#"CREATE TABLE "default010v1"."articles" ("id" uuid PRIMARY KEY, "title" text, "author_id" uuid REFERENCES "default010v1"."authors"("id"), "created_at" timestamptz, "updated_at" timestamptz)"#)
        .execute(pool).await.unwrap();

    common::create_meta_tables(pool).await;
    sqlx::query(
        r#"INSERT INTO "default010v1"."alcedocore_collection_definitions" (name, display_name)
           VALUES ('authors','Authors'), ('articles','Articles')"#,
    ).execute(pool).await.unwrap();
    common::add_field(pool, "authors", "id", "uuid", 1, None, None).await;
    common::add_field(pool, "authors", "name", "text", 2, None, None).await;
    common::add_field(pool, "authors", "country", "text", 3, None, None).await;
    common::add_field(pool, "articles", "id", "uuid", 1, None, None).await;
    common::add_field(pool, "articles", "title", "text", 2, None, None).await;
    common::add_field(pool, "articles", "author_id", "relationship", 3, Some("authors"), Some("many_to_one")).await;
}

#[tokio::test]
async fn execute_list_filters_sorts_and_paginates() {
    let (pool, _container) = common::start_postgres().await.unwrap();
    make_collections(&pool).await;

    let a1 = "aaaaaaaa-0000-0000-0000-000000000001";
    let a2 = "aaaaaaaa-0000-0000-0000-000000000002";
    sqlx::query(r#"INSERT INTO "default010v1"."authors" ("id","name","country") VALUES ($1::uuid,'Ann','NL'), ($2::uuid,'Zoe','US')"#)
        .bind(a1).bind(a2).execute(&pool).await.unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext { app_name: "default".into(), version: "v1".into(), request_source: RequestSource::FirstMigration };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "authors".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);
    let result = service
        .read_list(&pool, ListRequest {
            filter: None,
            sort: vec![SortField { field: "name".into(), order: "desc".into() }],
            limit: 1,
            offset: 0,
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(result.total, 2);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].get("name"), Some(&json!("Zoe")));
}

#[tokio::test]
async fn execute_list_applies_filter() {
    let (pool, _container) = common::start_postgres().await.unwrap();
    make_collections(&pool).await;
    sqlx::query(r#"INSERT INTO "default010v1"."authors" ("id","name","country") VALUES ($1::uuid,'Ann','NL'), ($2::uuid,'Zoe','US')"#)
        .bind("aaaaaaaa-0000-0000-0000-000000000001")
        .bind("aaaaaaaa-0000-0000-0000-000000000002")
        .execute(&pool).await.unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext { app_name: "default".into(), version: "v1".into(), request_source: RequestSource::FirstMigration };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "authors".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);
    let result = service
        .read_list(&pool, ListRequest {
            filter: Some(alcedo_db::db::filter_condition::FilterCondition::Rule {
                field: "country".into(),
                operator: alcedo_db::db::filter_condition::ComparisonOperator::Eq,
                value: Some(json!("NL")),
            }),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].get("name"), Some(&json!("Ann")));
}

#[tokio::test]
async fn execute_list_expands_nested_relation() {
    let (pool, _container) = common::start_postgres().await.unwrap();
    make_collections(&pool).await;
    sqlx::query(r#"INSERT INTO "default010v1"."authors" ("id","name","country") VALUES ('aaaaaaaa-0000-0000-0000-000000000001','Ann','NL')"#)
        .execute(&pool).await.unwrap();
    sqlx::query(r#"INSERT INTO "default010v1"."articles" ("id","title","author_id") VALUES ('bbbbbbbb-0000-0000-0000-000000000001','Hello','aaaaaaaa-0000-0000-0000-000000000001'::uuid)"#)
        .execute(&pool).await.unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext { app_name: "default".into(), version: "v1".into(), request_source: RequestSource::FirstMigration };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "articles".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);
    let result = service
        .read_list(&pool, ListRequest {
            fields: vec!["title".into(), "author_id.name".into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(result.total, 1);
    let author = result.items[0].get("author_id").expect("author_id should be expanded to an object");
    assert_eq!(author.get("name"), Some(&json!("Ann")));
}

#[tokio::test]
async fn execute_one_fetches_by_id() {
    let (pool, _container) = common::start_postgres().await.unwrap();
    make_collections(&pool).await;

    let author_id = "aaaaaaaa-0000-0000-0000-000000000001";
    let article_id = "bbbbbbbb-0000-0000-0000-000000000001";
    sqlx::query(r#"INSERT INTO "default010v1"."authors" ("id","name","country") VALUES ($1::uuid,'Ann','NL')"#)
        .bind(author_id).execute(&pool).await.unwrap();
    sqlx::query(r#"INSERT INTO "default010v1"."articles" ("id","title","author_id") VALUES ($1::uuid,'Hello',$2::uuid)"#)
        .bind(article_id).bind(author_id).execute(&pool).await.unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext { app_name: "default".into(), version: "v1".into(), request_source: RequestSource::FirstMigration };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "articles".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let found = service
        .read_one(&pool, OneRequest {
            item_id: article_id.to_string(),
            fields: vec!["title".into()],
            backlink: true,
            depth_limit: 5,
        })
        .await
        .unwrap()
        .expect("article should be found");
    assert_eq!(found.get("title"), Some(&json!("Hello")));
    assert!(found.get("id").is_some(), "implicit id column should be present");

    let missing = service
        .read_one(&pool, OneRequest {
            item_id: "00000000-0000-0000-0000-000000000000".to_string(),
            fields: vec!["title".into()],
            backlink: true,
            depth_limit: 5,
        })
        .await
        .unwrap();
    assert!(missing.is_none(), "non-existent id must return None");

    let err = service
        .read_one(&pool, OneRequest {
            item_id: article_id.to_string(),
            fields: vec!["nonexistent".into()],
            backlink: true,
            depth_limit: 5,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)), "expected BadRequest, got {:?}", err);
}

#[tokio::test]
async fn execute_list_augment_flag_controls_display_values() {
    let (pool, _container) = common::start_postgres().await.unwrap();
    make_collections(&pool).await;

    sqlx::query(
        r#"UPDATE "default010v1"."alcedocore_collection_fields" SET display_field='name' WHERE collection_name='articles' AND name='author_id'"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let author_id = "aaaaaaaa-0000-0000-0000-000000000001";
    let article_id = "bbbbbbbb-0000-0000-0000-000000000001";
    sqlx::query(r#"INSERT INTO "default010v1"."authors" ("id","name","country") VALUES ($1::uuid,'Ann','NL')"#)
        .bind(author_id).execute(&pool).await.unwrap();
    sqlx::query(r#"INSERT INTO "default010v1"."articles" ("id","title","author_id") VALUES ($1::uuid,'Hello',$2::uuid)"#)
        .bind(article_id).bind(author_id).execute(&pool).await.unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext { app_name: "default".into(), version: "v1".into(), request_source: RequestSource::FirstMigration };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "articles".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let augmented = service
        .read_list(&pool, ListRequest {
            fields: vec!["author_id".into()],
            augment: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(augmented.items.len(), 1);
    assert_eq!(
        augmented.items[0].get("author_id__display_value"),
        Some(&json!("Ann")),
        "augment=true should resolve relationship display values"
    );

    let raw = service
        .read_list(&pool, ListRequest {
            fields: vec!["author_id".into()],
            augment: false,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(raw.items.len(), 1);
    let has_display_key = raw.items[0]
        .as_object()
        .unwrap()
        .keys()
        .any(|k| k.ends_with("__display_value"));
    assert!(!has_display_key, "augment=false must not expand display values: {}", raw.items[0]);
}
