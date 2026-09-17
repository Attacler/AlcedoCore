#[path = "common/mod.rs"]
mod common;

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_common::error::AppError;
use alcedo_db::core_state_for_migrations;
use alcedo_db::db::collections::FieldType;
use alcedo_db::services::items::read::{execute_list_for_table, ListRequest};
use alcedo_db::services::items::shape::{
    column_cast, validate_fields_for_write_shape, ColumnShape, TableRef, TableShape,
};
use alcedo_db::services::items::write::execute_create_for_table;
use alcedo_db::services::tables::TableService;
use plugin_core::config::AppConfig;
use serde_json::json;

fn global_ctx() -> AppContext {
    AppContext {
        app_name: "alcedo".into(),
        version: "".into(),
        request_source: RequestSource::FirstMigration,
    }
}

/// Resolve a global `alcedo` table shape against a fresh test database,
/// mirroring the runtime flow (migrations + schema refresh + resolve).
async fn resolve_table(schema: &str, name: &str) -> TableShape {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = global_ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some(schema.into()),
            name: name.into(),
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn alcedo_users_shape_has_readable_allowlist() {
    let shape = resolve_table("alcedo", "alcedo_users").await;

    let readable = shape.readable.expect("readable allowlist should be present");
    assert!(readable.contains(&"id".to_string()));
    assert!(readable.contains(&"email".to_string()));
    assert!(
        !readable.contains(&"password_hash".to_string()),
        "readable allowlist must not expose password_hash: {:?}",
        readable
    );

    let writable = shape.writable.expect("writable allowlist should be present");
    assert!(writable.contains(&"email".to_string()));
    assert!(
        !writable.contains(&"password_hash".to_string()),
        "writable allowlist must not include password_hash: {:?}",
        writable
    );
}

#[tokio::test]
async fn allowlisted_tables_exclude_sensitive_columns_from_readable() {
    let cases: &[(&str, &[&str])] = &[
        ("alcedo_users", &["password_hash"]),
        ("alcedo_developer_api_keys", &["key_hash"]),
        ("alcedo_registries", &["username", "password"]),
        ("alcedo_plugins", &["env"]),
    ];

    for (table, sensitive) in cases {
        let shape = resolve_table("alcedo", table).await;

        let readable = shape
            .readable
            .as_ref()
            .unwrap_or_else(|| panic!("{} must carry a readable allowlist", table));

        for column in *sensitive {
            assert!(
                !readable.contains(&column.to_string()),
                "{} readable allowlist must not expose {}: {:?}",
                table,
                column,
                readable
            );
        }
    }

    let users = resolve_table("alcedo", "alcedo_users").await;
    let writable = users
        .writable
        .as_ref()
        .unwrap_or_else(|| panic!("alcedo_users must carry a writable allowlist"));
    assert!(
        !writable.contains(&"password_hash".to_string()),
        "alcedo_users writable allowlist must not include password_hash: {:?}",
        writable
    );
}

#[tokio::test]
async fn alcedo_versions_readable_is_none() {
    let shape = resolve_table("alcedo", "alcedo_versions").await;

    assert_eq!(shape.readable, None, "unlisted tables keep * projections");
    assert_eq!(shape.writable, None, "unlisted tables keep permissive writes");
}

#[tokio::test]
async fn empty_fields_on_sensitive_table_never_projects_secrets() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = global_ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    sqlx::query(
        r#"INSERT INTO alcedo.alcedo_users (id, email, password_hash, display_name, is_admin)
           VALUES (gen_random_uuid(), $1, $2, $3, false)"#,
    )
    .bind("allowlist@example.com")
    .bind("super-secret-hash")
    .bind("Allowlist User")
    .execute(&pool)
    .await
    .unwrap();

    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some("alcedo".into()),
            name: "alcedo_users".into(),
        },
    )
    .await
    .unwrap();

    let result = execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            fields: vec![],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    let item = result
        .items
        .first()
        .expect("one item should be returned");
    let obj = item.as_object().expect("item should be an object");

    assert!(
        !obj.contains_key("password_hash"),
        "empty-field projection leaked password_hash: {:?}",
        obj
    );
    assert!(obj.contains_key("id"), "id should be projected: {:?}", obj);
    assert!(
        obj.contains_key("email"),
        "email should be projected: {:?}",
        obj
    );
    assert_eq!(obj.get("email"), Some(&serde_json::json!("allowlist@example.com")));
}

#[tokio::test]
async fn writable_allowlist_rejects_password_hash() {
    let shape = resolve_table("alcedo", "alcedo_users").await;

    let err = validate_fields_for_write_shape(&["password_hash".to_string()], &shape).unwrap_err();
    assert!(
        matches!(err, AppError::BadRequest(ref msg) if msg.contains("not writable")),
        "got {:?}",
        err
    );

    validate_fields_for_write_shape(&["email".to_string()], &shape)
        .expect("allowlisted writable field should pass");
}

#[test]
fn column_cast_derives_physical_casts_from_field_type_and_data_type() {
    let col = |data_type: &str, field_type: FieldType| ColumnShape {
        name: "c".into(),
        field_type,
        data_type: data_type.into(),
        is_nullable: true,
        is_unique: false,
        is_pk: false,
        has_default: false,
        has_auto_increment: false,
        foreign_key: None,
    };

    assert_eq!(column_cast(&col("jsonb", FieldType::String)), "::jsonb");
    assert_eq!(column_cast(&col("json", FieldType::String)), "::jsonb");
    assert_eq!(column_cast(&col("text[]", FieldType::String)), "::text[]");
    assert_eq!(column_cast(&col("integer[]", FieldType::String)), "::bigint[]");
    assert_eq!(column_cast(&col("boolean[]", FieldType::String)), "::bool[]");
    assert_eq!(column_cast(&col("uuid[]", FieldType::File)), "::uuid[]");
    assert_eq!(column_cast(&col("text[]", FieldType::File)), "::text[]");
    assert_eq!(column_cast(&col("integer[]", FieldType::File)), "::bigint[]");
    assert_eq!(column_cast(&col("boolean[]", FieldType::File)), "::bool[]");
    assert_eq!(column_cast(&col("text", FieldType::String)), "");
    assert_eq!(column_cast(&col("uuid", FieldType::Uuid)), "::uuid");
    assert_eq!(column_cast(&col("integer", FieldType::Int)), "::bigint");
}

#[tokio::test]
async fn physical_json_and_array_write_read() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = global_ctx();

    sqlx::query(
        r#"CREATE TABLE alcedo.test_phase4_types (
            id SERIAL PRIMARY KEY,
            data JSONB NOT NULL,
            tags TEXT[] NOT NULL DEFAULT '{}'
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    TableService::new(&core, &ctx).refresh_schema().await;

    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some("alcedo".into()),
            name: "test_phase4_types".into(),
        },
    )
    .await
    .unwrap();

    let data = shape
        .columns
        .iter()
        .find(|c| c.name == "data")
        .expect("data column resolved");
    let tags = shape
        .columns
        .iter()
        .find(|c| c.name == "tags")
        .expect("tags column resolved");
    assert_eq!(data.field_type, FieldType::String);
    assert_eq!(tags.field_type, FieldType::File);
    assert_eq!(data.data_type, "jsonb");
    assert_eq!(tags.data_type, "text[]");

    let mut map = serde_json::Map::new();
    map.insert("data".into(), json!({"a": 1}));
    map.insert("tags".into(), json!(["x", "y"]));
    let outcome = execute_create_for_table(&pool, &shape, vec![map])
        .await
        .expect("insert into jsonb/text[] table should succeed");
    assert_eq!(outcome.affected.len(), 1);

    let result = execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            fields: vec!["data".into(), "tags".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    let item = result.items.first().expect("one item should be returned");
    assert_eq!(item.get("data"), Some(&json!({"a": 1})));
    assert_eq!(item.get("tags"), Some(&json!(["x", "y"])));
}
