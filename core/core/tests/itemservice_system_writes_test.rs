#[path = "common/mod.rs"]
mod common;

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_common::error::AppError;
use alcedo_db::core_state_for_migrations;
use alcedo_db::db::collection_items::{CreateItemsBody, DeleteItemsBody, UpdateItemsBody};
use alcedo_db::db::{ALCEDO_SCHEMA, DEFAULT_APP_VERSION_SCHEMA};
use alcedo_db::services::items::read::{execute_list_for_table, ListRequest};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::{
    validate_fields_for_write_shape, TableRef, TableShape,
};
use alcedo_db::services::items::write::{
    execute_create_for_table, execute_insert_for_table_with_conflict, ConflictPolicy,
};
use alcedo_db::services::tables::TableService;
use plugin_core::config::AppConfig;
use serde_json::{json, Map, Value};

/// Build a live pool with the global `alcedo` schema migrated + the schema
/// cache refreshed, plus a global context suitable for
/// `ItemsService::for_global`. Each call owns its own disposable database so
/// tests never collide.
async fn core_setup() -> (
    common::TestDb,
    sqlx::PgPool,
    alcedo_db::CoreState,
    AppContext,
) {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext {
        app_name: ALCEDO_SCHEMA.into(),
        version: "".into(),
        request_source: RequestSource::FirstMigration,
    };
    TableService::new(&core, &ctx).refresh_schema().await;
    (db, pool, core, ctx)
}

#[tokio::test]
async fn bulk_update_physical_dispatches_and_updates_matching_rows() {
    let (_db, pool, core, _ctx) = core_setup().await;
    let collection = "alcedo_versions".to_string();
    let service = ItemsService::for_global(&core, &collection);

    for name in ["alpha", "beta"] {
        let mut m = Map::new();
        m.insert("version_name".into(), Value::String(name.into()));
        service.create(&pool, CreateItemsBody::Single(m)).await.unwrap();
    }

    let mut update = Map::new();
    update.insert("version_name".into(), Value::String("renamed".into()));
    let body = UpdateItemsBody {
        filter: json!({ "version_name": { "_eq": "alpha" } }),
        update,
    };
    let outcome = service.update(&pool, body, None).await.unwrap();
    assert_eq!(outcome.affected_count, 1);

    let result = service
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(ALCEDO_SCHEMA.into()),
                name: "alcedo_versions".into(),
            },
            ListRequest {
                fields: vec!["version_name".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let names: Vec<String> = result
        .items
        .iter()
        .filter_map(|i| i.get("version_name").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();
    assert!(names.contains(&"renamed".to_string()));
    assert!(names.contains(&"beta".to_string()));
}

#[tokio::test]
async fn filter_delete_physical_removes_matching_rows() {
    let (_db, pool, core, _ctx) = core_setup().await;
    let collection = "alcedo_versions".to_string();
    let service = ItemsService::for_global(&core, &collection);
    let mut m = Map::new();
    m.insert("version_name".into(), Value::String("doomed".into()));
    service.create(&pool, CreateItemsBody::Single(m)).await.unwrap();

    let outcome = service
        .delete(
            &pool,
            DeleteItemsBody {
                filter: Some(json!({ "version_name": { "_eq": "doomed" } })),
                pk_values: None,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(outcome.affected_count, 1);
    let result = service
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(ALCEDO_SCHEMA.into()),
                name: "alcedo_versions".into(),
            },
            ListRequest {
                fields: vec!["version_name".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        !result
            .items
            .iter()
            .any(|i| i.get("version_name") == Some(&Value::String("doomed".into())))
    );
}

#[tokio::test]
async fn insert_ignore_and_upsert_respect_conflict_columns() {
    let (_db, pool, core, ctx) = core_setup().await;
    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some(DEFAULT_APP_VERSION_SCHEMA.into()),
            name: "alcedocore_system_settings".into(),
        },
    )
    .await
    .unwrap();

    let mut m1 = Map::new();
    m1.insert("key".into(), Value::String("k".into()));
    m1.insert("value".into(), json!({"a": 1}));
    execute_insert_for_table_with_conflict(&pool, &shape, &["key"], ConflictPolicy::Upsert, vec![m1])
        .await
        .unwrap();

    let mut m2 = Map::new();
    m2.insert("key".into(), Value::String("k".into()));
    m2.insert("value".into(), json!({"a": 2}));
    let out = execute_insert_for_table_with_conflict(
        &pool,
        &shape,
        &["key"],
        ConflictPolicy::Upsert,
        vec![m2],
    )
    .await
    .unwrap();
    assert_eq!(out.affected.len(), 1);
    assert_eq!(out.affected[0]["value"], json!({"a": 2}));
}

#[tokio::test]
async fn privileged_write_shape_allows_password_hash() {
    let (_db, pool, core, _ctx) = core_setup().await;
    let collection = "alcedo_users".to_string();
    let service = ItemsService::for_global(&core, &collection);
    let shape = service
        .privileged_write_shape(
            TableRef {
                schema: Some(ALCEDO_SCHEMA.into()),
                name: "alcedo_users".into(),
            },
            &["password_hash"],
        )
        .await
        .unwrap();
    let mut m = Map::new();
    m.insert("email".into(), Value::String("p@example.com".into()));
    m.insert("password_hash".into(), Value::String("x".into()));
    let out = execute_create_for_table(&pool, &shape, vec![m]).await.unwrap();
    assert_eq!(out.affected[0]["email"], "p@example.com");
}

#[tokio::test]
async fn roles_allowlist_rejects_is_system_write() {
    let (_db, _pool, core, ctx) = core_setup().await;
    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some(DEFAULT_APP_VERSION_SCHEMA.into()),
            name: "alcedocore_roles".into(),
        },
    )
    .await
    .unwrap();
    let err = validate_fields_for_write_shape(&["is_system".to_string()], &shape).unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));

    validate_fields_for_write_shape(&["name".to_string(), "description".to_string()], &shape)
        .expect("writable roles columns should pass validation");
}

#[tokio::test]
async fn scalar_jsonb_values_round_trip_through_engine() {
    let (_db, pool, core, ctx) = core_setup().await;
    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some(DEFAULT_APP_VERSION_SCHEMA.into()),
            name: "alcedocore_system_settings".into(),
        },
    )
    .await
    .unwrap();

    // Scalar jsonb values (string, number, bool) must survive the engine's
    // JSON-encoding bind path and read back equal.
    let scalars: Vec<(&str, Value)> = vec![
        ("k-str", json!("hello")),
        ("k-num", json!(42)),
        ("k-float", json!(3.5)),
        ("k-bool-true", json!(true)),
        ("k-bool-false", json!(false)),
    ];
    let items: Vec<Map<String, Value>> = scalars
        .iter()
        .map(|(k, v)| {
            let mut m = Map::new();
            m.insert("key".into(), Value::String(k.to_string()));
            m.insert("value".into(), v.clone());
            m
        })
        .collect();
    execute_insert_for_table_with_conflict(&pool, &shape, &["key"], ConflictPolicy::Upsert, items)
        .await
        .unwrap();

    let result = execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            fields: vec!["key".into(), "value".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for (k, v) in &scalars {
        let item = result
            .items
            .iter()
            .find(|i| i.get("key") == Some(&Value::String(k.to_string())))
            .unwrap_or_else(|| panic!("scalar setting {} should exist", k));
        assert_eq!(
            item.get("value"),
            Some(v),
            "scalar jsonb value for {} should round-trip",
            k
        );
    }
}

#[tokio::test]
async fn do_nothing_conflict_is_idempotent_noop() {
    let (_db, pool, core, ctx) = core_setup().await;
    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some(DEFAULT_APP_VERSION_SCHEMA.into()),
            name: "alcedocore_system_settings".into(),
        },
    )
    .await
    .unwrap();

    let mut m1 = Map::new();
    m1.insert("key".into(), Value::String("k-do-nothing".into()));
    m1.insert("value".into(), json!({"v": 1}));
    execute_insert_for_table_with_conflict(&pool, &shape, &["key"], ConflictPolicy::Upsert, vec![m1])
        .await
        .unwrap();

    let mut m2 = Map::new();
    m2.insert("key".into(), Value::String("k-do-nothing".into()));
    m2.insert("value".into(), json!({"v": 2}));
    let out = execute_insert_for_table_with_conflict(
        &pool,
        &shape,
        &["key"],
        ConflictPolicy::DoNothing,
        vec![m2],
    )
    .await
    .expect("conflicting DoNothing insert must not error");
    assert_eq!(out.affected.len(), 0, "conflicting DoNothing insert must no-op");

    let result = execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            fields: vec!["key".into(), "value".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let item = result
        .items
        .iter()
        .find(|i| i.get("key") == Some(&Value::String("k-do-nothing".into())))
        .expect("row should still exist");
    assert_eq!(item.get("value"), Some(&json!({"v": 1})));
}
