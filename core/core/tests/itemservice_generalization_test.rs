#[path = "common/mod.rs"]
mod common;

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_common::error::AppError;
use alcedo_db::core_state_for_migrations;
use alcedo_db::db::collection_items::{CreateItemsBody, DeleteItemsBody};
use alcedo_db::db::collections::{CollectionDefinition, FieldDefinition, FieldType};
use alcedo_db::db::field_resolver::{
    detect_direction, resolve_nested_fields, Direction, FieldResolverOptions,
};
use alcedo_db::db::filter_condition::{
    ComparisonOperator, FilterCondition, GroupedQueryRequest, SortField,
};
use alcedo_db::services::items::read::{execute_grouped_for_table, ListRequest, OneRequest};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::{
    validate_fields_for_write_shape, ColumnShape, PhysicalCatalog, TableRef, TableShape,
};
use alcedo_db::services::tables::TableService;
use plugin_core::config::AppConfig;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;

fn ctx() -> AppContext {
    AppContext {
        app_name: "default".into(),
        version: "v1".into(),
        request_source: RequestSource::FirstMigration,
    }
}

fn global_ctx() -> AppContext {
    AppContext {
        app_name: "alcedo".into(),
        version: "".into(),
        request_source: RequestSource::FirstMigration,
    }
}

fn empty_def(name: &str) -> CollectionDefinition {
    CollectionDefinition {
        name: name.to_string(),
        display_name: None,
        fields: Vec::new(),
        is_system: false,
        plugin_slug: None,
        created_at: None,
        updated_at: None,
    }
}

#[tokio::test]
async fn resolve_pg_only_table() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some("alcedo".into()),
            name: "alcedo_versions".into(),
        },
    )
    .await
    .unwrap();

    assert_eq!(shape.schema, "alcedo");
    assert_eq!(shape.name, "alcedo_versions");
    assert_eq!(shape.pk, vec!["id".to_string()]);
    assert!(shape.collection.is_none());

    let id_col = shape
        .columns
        .iter()
        .find(|c| c.name == "id")
        .expect("id column");
    assert_eq!(id_col.field_type, FieldType::Int);
    assert!(id_col.is_pk);
    assert!(id_col.has_default);
    assert!(!id_col.is_nullable);
}

#[tokio::test]
async fn global_schema_resolution_stays_physical() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = global_ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    // TestDb seeds an app-scoped system collection literally named
    // `alcedo_users`; resolving the global `alcedo.alcedo_users` table must
    // ignore it and stay physical.
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

    assert_eq!(shape.schema, "alcedo");
    assert_eq!(shape.name, "alcedo_users");
    assert!(
        shape.collection.is_none(),
        "global alcedo tables must resolve physically, got metadata: {:?}",
        shape.collection
    );
}

#[tokio::test]
async fn resolve_metadata_collection_and_overrides_types() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();

    sqlx::query(
        r#"CREATE TABLE "default010v1"."general_shape_authors" (
             "id" uuid PRIMARY KEY,
             "name" text,
             "score" int,
             "attachment_refs" uuid[],
             "version_ref" int REFERENCES "alcedo"."alcedo_versions"("id"),
             "created_at" timestamptz,
             "updated_at" timestamptz
           )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    common::create_meta_tables(&pool).await;
    sqlx::query(
        r#"INSERT INTO "default010v1"."alcedocore_collection_definitions" (name, display_name)
           VALUES ('general_shape_authors','Authors')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    common::add_field(&pool, "general_shape_authors", "id", "uuid", 1, None, None).await;
    common::add_field(
        &pool,
        "general_shape_authors",
        "name",
        "text",
        2,
        None,
        None,
    )
    .await;
    common::add_field(
        &pool,
        "general_shape_authors",
        "score",
        "string",
        3,
        None,
        None,
    )
    .await;
    common::add_field(
        &pool,
        "general_shape_authors",
        "version_ref",
        "relationship",
        4,
        Some("versions"),
        Some("many_to_one"),
    )
    .await;
    common::add_field(
        &pool,
        "general_shape_authors",
        "created_at",
        "datetime",
        5,
        None,
        None,
    )
    .await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: None,
            name: "general_shape_authors".into(),
        },
    )
    .await
    .unwrap();

    assert_eq!(shape.schema, "default010v1");
    assert_eq!(shape.name, "general_shape_authors");
    assert_eq!(shape.pk, vec!["id".to_string()]);
    let collection = shape
        .collection
        .as_ref()
        .expect("collection metadata should be attached");
    assert_eq!(collection.name, "general_shape_authors");

    let map = shape.col_type_map_ref();
    assert_eq!(*map["id"], FieldType::Uuid);
    assert_eq!(*map["created_at"], FieldType::Datetime);
    assert_eq!(*map["name"], FieldType::Text);
    assert_eq!(
        *map["score"],
        FieldType::String,
        "metadata field type must override the physical int"
    );
    assert_eq!(
        *map["attachment_refs"],
        FieldType::File,
        "physical array maps to File"
    );

    let version_ref = shape
        .columns
        .iter()
        .find(|c| c.name == "version_ref")
        .unwrap();
    let fk = version_ref
        .foreign_key
        .as_ref()
        .expect("foreign key should be captured");
    assert_eq!(fk.table, "alcedo_versions");
    assert_eq!(fk.column, "id");
    assert_eq!(fk.schema, "alcedo");
}

#[tokio::test]
async fn resolve_missing_table_returns_not_found() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let err = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: None,
            name: "does_not_exist".into(),
        },
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, AppError::NotFound(_)),
        "expected NotFound, got {:?}",
        err
    );
}

#[tokio::test]
async fn physical_fk_resolves_without_collection_definitions() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();

    sqlx::query(
        r#"CREATE TABLE "default010v1"."phys_authors" (
             "id" uuid PRIMARY KEY,
             "name" text
           )"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"CREATE TABLE "default010v1"."phys_articles" (
             "id" uuid PRIMARY KEY,
             "title" text,
             "author_id" uuid REFERENCES "default010v1"."phys_authors"("id")
           )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let schema = core.schema.read().await;
    let catalog = PhysicalCatalog::from_schema(&schema);
    drop(schema);

    let articles = empty_def("phys_articles");
    let direction = detect_direction(
        "author_id",
        "phys_articles",
        &articles,
        &[],
        Some("default010v1"),
        Some(&catalog),
    )
    .unwrap();
    match direction {
        Direction::ManyToOne {
            target_collection,
            target_schema,
            fk_column,
            target_pk_column,
        } => {
            assert_eq!(target_collection, "phys_authors");
            assert_eq!(target_schema.as_deref(), Some("default010v1"));
            assert_eq!(fk_column, "author_id");
            assert_eq!(target_pk_column, "id");
        }
        _ => panic!("expected ManyToOne for physical FK column"),
    }

    let authors = empty_def("phys_authors");
    let reverse = detect_direction(
        "phys_articles",
        "phys_authors",
        &authors,
        &[],
        Some("default010v1"),
        Some(&catalog),
    )
    .unwrap();
    match reverse {
        Direction::OneToMany {
            target_collection,
            target_schema,
            fk_column,
            base_pk_column,
        } => {
            assert_eq!(target_collection, "phys_articles");
            assert_eq!(target_schema.as_deref(), Some("default010v1"));
            assert_eq!(fk_column, "author_id");
            assert_eq!(base_pk_column, "id");
        }
        _ => panic!("expected OneToMany for reverse physical FK"),
    }
}

#[tokio::test]
async fn physical_fk_resolves_cross_schema() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let schema = core.schema.read().await;
    let catalog = PhysicalCatalog::from_schema(&schema);
    drop(schema);

    let user_roles = empty_def("alcedocore_user_roles");
    let direction = detect_direction(
        "user_id",
        "alcedocore_user_roles",
        &user_roles,
        &[],
        Some("default010v1"),
        Some(&catalog),
    )
    .unwrap();

    match direction {
        Direction::ManyToOne {
            target_collection,
            target_schema,
            fk_column,
            target_pk_column,
        } => {
            assert_eq!(target_collection, "alcedo_users");
            assert_eq!(target_schema.as_deref(), Some("alcedo"));
            assert_eq!(fk_column, "user_id");
            assert_eq!(target_pk_column, "id");
        }
        _ => panic!("expected ManyToOne for cross-schema physical FK"),
    }
}

async fn create_cross_schema_links_fixture() -> (
    common::TestDb,
    Vec<CollectionDefinition>,
    PhysicalCatalog,
) {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();

    sqlx::query(
        r#"CREATE TABLE "default010v1"."cross_schema_links" (
             "id" uuid PRIMARY KEY,
             "user_id" uuid REFERENCES "alcedo"."alcedo_users"("id"),
             "created_at" timestamptz,
             "updated_at" timestamptz
           )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    common::create_meta_tables(&pool).await;
    sqlx::query(
        r#"INSERT INTO "default010v1"."alcedocore_collection_definitions" (name, display_name)
           VALUES ('cross_schema_links','Cross Schema Links')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    common::add_field(&pool, "cross_schema_links", "id", "uuid", 1, None, None).await;
    common::add_field(
        &pool,
        "cross_schema_links",
        "user_id",
        "relationship",
        2,
        Some("alcedo_users"),
        Some("many_to_one"),
    )
    .await;
    common::add_field(
        &pool,
        "cross_schema_links",
        "created_at",
        "datetime",
        3,
        None,
        None,
    )
    .await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let collections = alcedo_db::db::collections::list_collections(&pool)
        .await
        .unwrap();
    let schema = core.schema.read().await;
    let catalog = PhysicalCatalog::from_schema(&schema);
    drop(schema);

    (db, collections, catalog)
}

#[tokio::test]
async fn nested_read_qualifies_cross_schema_relation_join() {
    let (_db, collections, catalog) = create_cross_schema_links_fixture().await;
    let fields = vec!["user_id.id".to_string()];

    let mut options = FieldResolverOptions {
        depth_limit: 5,
        backlink: true,
        visited: HashSet::new(),
        schema: Some("default010v1".to_string()),
        physical: Some(Arc::new(catalog)),
    };
    let fragments =
        resolve_nested_fields(&fields, "cross_schema_links", &collections, &mut options).unwrap();
    assert_eq!(fragments.len(), 1);
    let clause = &fragments[0].select_clause;
    assert!(
        clause.contains(r#""alcedo"."alcedo_users""#),
        "expected schema-qualified join target, got: {}",
        clause
    );
    assert!(
        !clause.contains(r#"FROM "alcedo_users" AS"#),
        "qualified join must not use a bare target table: {}",
        clause
    );

    let mut schema_only = FieldResolverOptions {
        depth_limit: 5,
        backlink: true,
        visited: HashSet::new(),
        schema: Some("default010v1".to_string()),
        physical: None,
    };
    let bare_with_schema =
        resolve_nested_fields(&fields, "cross_schema_links", &collections, &mut schema_only)
            .unwrap();
    assert_eq!(bare_with_schema.len(), 1);
    assert!(
        bare_with_schema[0]
            .select_clause
            .contains(r#"FROM "alcedo_users" AS"#),
        "schema alone must not qualify without catalog metadata: {}",
        bare_with_schema[0].select_clause
    );

    let mut bare_options = FieldResolverOptions {
        depth_limit: 5,
        backlink: true,
        visited: HashSet::new(),
        schema: None,
        physical: None,
    };
    let bare =
        resolve_nested_fields(&fields, "cross_schema_links", &collections, &mut bare_options)
            .unwrap();
    assert_eq!(bare.len(), 1);
    assert!(
        bare[0].select_clause.contains(r#"FROM "alcedo_users" AS"#),
        "without schema context the target must stay unqualified: {}",
        bare[0].select_clause
    );
}

#[tokio::test]
async fn filter_join_qualifies_cross_schema_relation() {
    let (_db, collections, catalog) = create_cross_schema_links_fixture().await;

    let mut joins = Vec::new();
    let field_ref = alcedo_db::db::filter_compiler::resolve_field_path(
        "user_id.id",
        "cross_schema_links",
        &collections,
        &mut joins,
        Some(&catalog),
        Some("default010v1"),
    )
    .unwrap();

    assert_eq!(field_ref, r#""_rel_user_id"."id""#);
    assert_eq!(joins.len(), 1);
    assert!(
        joins[0].contains(r#""alcedo"."alcedo_users""#),
        "expected schema-qualified filter join, got: {}",
        joins[0]
    );
    assert!(
        !joins[0].contains(r#"JOIN "alcedo_users" AS"#),
        "qualified filter join must not use a bare target table: {}",
        joins[0]
    );

    let mut bare_joins = Vec::new();
    alcedo_db::db::filter_compiler::resolve_field_path(
        "user_id.id",
        "cross_schema_links",
        &collections,
        &mut bare_joins,
        None,
        None,
    )
    .unwrap();

    assert_eq!(bare_joins.len(), 1);
    assert!(
        bare_joins[0].contains(r#"JOIN "alcedo_users" AS"#),
        "without catalog metadata the filter join must stay unqualified: {}",
        bare_joins[0]
    );
}

fn text_field(name: &str) -> FieldDefinition {
    FieldDefinition {
        name: name.into(),
        field_type: FieldType::Text,
        required: false,
        unique: false,
        default: None,
        display_name: None,
        display_type: None,
        input_component: None,
        display_component: None,
        options: None,
        is_system: false,
        hidden: false,
        related_collection: None,
        relationship_type: None,
        display_field: None,
        inline_parent_fields: None,
    }
}

#[tokio::test]
async fn flat_read_list_and_one_on_global_table() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();

    sqlx::query(
        r#"INSERT INTO "alcedo"."alcedo_versions" (version_name) VALUES ('v1'), ('v2'), ('v3')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let reference = TableRef {
        schema: Some("alcedo".into()),
        name: "alcedo_versions".into(),
    };
    let shape = TableShape::resolve(&core, &ctx, reference.clone())
        .await
        .unwrap();
    assert!(shape.collection.is_none());

    let collection = "alcedo_versions".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let result = service
        .read_list_for_table(
            &pool,
            reference.clone(),
            ListRequest {
                sort: vec![SortField {
                    field: "id".into(),
                    order: "asc".into(),
                }],
                limit: 2,
                offset: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(result.total, 3);
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].get("version_name"), Some(&json!("v2")));
    assert_eq!(result.items[1].get("version_name"), Some(&json!("v3")));

    let filtered = alcedo_db::services::items::read::execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            filter: Some(FilterCondition::Rule {
                field: "version_name".into(),
                operator: ComparisonOperator::Eq,
                value: Some(json!("v2")),
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.items.len(), 1);
    assert_eq!(filtered.items[0].get("version_name"), Some(&json!("v2")));

    let one = service
        .read_one_for_table(
            &pool,
            reference,
            OneRequest {
                item_id: "2".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .expect("row 2 should exist");
    assert_eq!(one.get("version_name"), Some(&json!("v2")));
}

#[test]
fn validate_fields_for_write_shape_rejects_unknown_on_physical_shape() {
    let db_free_shape = TableShape {
        schema: "alcedo".into(),
        name: "alcedo_versions".into(),
        columns: vec![
            ColumnShape {
                name: "id".into(),
                field_type: FieldType::Int,
                data_type: String::new(),
                is_nullable: false,
                is_unique: true,
                is_pk: true,
                has_default: true,
                has_auto_increment: true,
                foreign_key: None,
            },
            ColumnShape {
                name: "version_name".into(),
                field_type: FieldType::Text,
                data_type: String::new(),
                is_nullable: false,
                is_unique: false,
                is_pk: false,
                has_default: false,
                has_auto_increment: false,
                foreign_key: None,
            },
        ],
        pk: vec!["id".into()],
        readable: None,
        writable: None,
        collection: None,
    };

    assert!(validate_fields_for_write_shape(&["version_name".to_string()], &db_free_shape).is_ok());
    assert!(
        validate_fields_for_write_shape(&["id".to_string()], &db_free_shape).is_ok(),
        "physical PK columns are writable; access is gated at the boundary"
    );

    let err = validate_fields_for_write_shape(&["nope".to_string()], &db_free_shape).unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)), "got {:?}", err);
}

#[test]
fn validate_fields_for_write_shape_delegates_reserved_rules_for_collections() {
    let mut collection = empty_def("authors");
    collection.fields = vec![text_field("name")];

    let shape = TableShape {
        schema: "default010v1".into(),
        name: "authors".into(),
        columns: vec![ColumnShape {
            name: "name".into(),
            field_type: FieldType::Text,
            data_type: String::new(),
            is_nullable: true,
            is_unique: false,
            is_pk: false,
            has_default: false,
            has_auto_increment: false,
            foreign_key: None,
        }],
        pk: vec!["id".into()],
        readable: None,
        writable: None,
        collection: Some(collection),
    };

    assert!(validate_fields_for_write_shape(&["name".to_string()], &shape).is_ok());

    let err = validate_fields_for_write_shape(&["id".to_string()], &shape).unwrap_err();
    assert!(
        matches!(err, AppError::BadRequest(ref msg) if msg.contains("reserved system column")),
        "got {:?}",
        err
    );
}

#[tokio::test]
async fn flat_read_path_projection_and_guardrails() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();

    sqlx::query(
        r#"INSERT INTO "alcedo"."alcedo_versions" (version_name) VALUES ('v1'), ('v2')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some("alcedo".into()),
            name: "alcedo_versions".into(),
        },
    )
    .await
    .unwrap();

    let projected = alcedo_db::services::items::read::execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            fields: vec!["version_name".into()],
            sort: vec![SortField {
                field: "id".into(),
                order: "asc".into(),
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(projected.items.len(), 2);
    assert_eq!(
        projected.items[0].get("id"),
        Some(&json!(1)),
        "flat projection must union the primary key"
    );
    assert_eq!(projected.items[0].get("version_name"), Some(&json!("v1")));

    let permissions_err = alcedo_db::services::items::read::execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            permissions: vec![alcedo_db::services::permissions::PolicyPermission {
                id: uuid::Uuid::nil(),
                policy_id: uuid::Uuid::nil(),
                collection_name: "alcedo_versions".into(),
                action: "read".into(),
                fields: None,
                filter: json!({}),
                field_validation: None,
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(permissions_err, AppError::BadRequest(_)),
        "got {:?}",
        permissions_err
    );

    let dotted_err = alcedo_db::services::items::read::execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            fields: vec!["version_name.id".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(dotted_err, AppError::BadRequest(_)),
        "got {:?}",
        dotted_err
    );

    let unknown_err = alcedo_db::services::items::read::execute_list_for_table(
        &pool,
        &shape,
        ListRequest {
            fields: vec!["nope".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(unknown_err, AppError::BadRequest(_)),
        "got {:?}",
        unknown_err
    );

    let missing = alcedo_db::services::items::read::execute_one_for_table(
        &pool,
        &shape,
        OneRequest {
            item_id: "9999".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(missing.is_none(), "missing id should return None");
}

#[tokio::test]
async fn write_shape_path_round_trips_integer_pk() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = global_ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "alcedo_versions".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);
    let reference = TableRef {
        schema: Some("alcedo".into()),
        name: "alcedo_versions".into(),
    };

    let mut create_map = serde_json::Map::new();
    create_map.insert("version_name".into(), json!("v-int"));
    let created = service
        .create(&pool, CreateItemsBody::Single(create_map))
        .await
        .unwrap();
    assert_eq!(created.affected.len(), 1, "expected one created row");
    assert_eq!(
        created.affected[0].get("version_name"),
        Some(&json!("v-int"))
    );
    let id = created.affected[0]
        .get("id")
        .and_then(|v| v.as_i64())
        .expect("serial primary key should come back as an integer")
        .to_string();

    let one = service
        .read_one_for_table(
            &pool,
            reference.clone(),
            OneRequest {
                item_id: id.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .expect("created row should be readable by integer pk");
    assert_eq!(one.get("version_name"), Some(&json!("v-int")));
    assert_eq!(one.get("id").and_then(|v| v.as_i64()), id.parse::<i64>().ok());

    let mut update_map = serde_json::Map::new();
    update_map.insert("version_name".into(), json!("v-int-renamed"));
    let updated = service
        .update_one(&pool, &id, &update_map, &[])
        .await
        .unwrap();
    assert_eq!(updated.affected.len(), 1);
    assert_eq!(
        updated.affected[0].get("version_name"),
        Some(&json!("v-int-renamed"))
    );
    assert_eq!(
        updated.affected[0].get("id").and_then(|v| v.as_i64()),
        id.parse::<i64>().ok()
    );
    assert_eq!(updated.pairs.len(), 1, "expected one old/new pair");
    assert_eq!(
        updated.pairs[0].0.get("version_name"),
        Some(&json!("v-int"))
    );

    let deleted = service
        .delete(
            &pool,
            DeleteItemsBody {
                filter: None,
                pk_values: Some(vec![json!(id.parse::<i64>().unwrap())]),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(deleted.affected_count, 1);
    assert_eq!(deleted.deleted.len(), 1);
    assert_eq!(
        deleted.deleted[0].get("version_name"),
        Some(&json!("v-int-renamed"))
    );
    assert_eq!(
        deleted.deleted[0].get("id").and_then(|v| v.as_i64()),
        id.parse::<i64>().ok()
    );

    let gone = service
        .read_one_for_table(
            &pool,
            reference,
            OneRequest {
                item_id: id,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(gone.is_none(), "deleted row should no longer be readable");
}

async fn create_phys_events_table(pool: &sqlx::PgPool) {
    sqlx::query(
        r#"CREATE TABLE "default010v1"."phys_events" (
             "id" serial PRIMARY KEY,
             "name" text,
             "happened_at" timestamptz
           )"#,
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn physical_create_persists_nullable_timestamp() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    create_phys_events_table(&pool).await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "phys_events".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let mut map = serde_json::Map::new();
    map.insert("name".into(), json!("deploy"));
    map.insert("happened_at".into(), json!("2024-01-02T03:04:05Z"));
    let created = service
        .create(&pool, CreateItemsBody::Single(map))
        .await
        .unwrap();

    assert_eq!(created.affected.len(), 1);
    assert_eq!(created.affected[0].get("name"), Some(&json!("deploy")));
    assert!(
        created.affected[0]
            .get("happened_at")
            .is_some_and(|v| !v.is_null()),
        "supplied timestamp must be returned, got {:?}",
        created.affected[0].get("happened_at")
    );

    let stored: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "default010v1"."phys_events" WHERE "happened_at" IS NOT NULL"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, 1, "supplied timestamp must be persisted");
}

#[tokio::test]
async fn physical_create_batch_rolls_back_on_invalid_item() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    create_phys_events_table(&pool).await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "phys_events".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let mut first = serde_json::Map::new();
    first.insert("name".into(), json!("first"));
    let mut second = serde_json::Map::new();
    second.insert("nope".into(), json!("boom"));

    let err = service
        .create(&pool, CreateItemsBody::Multiple(vec![first, second]))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)), "got {:?}", err);

    let count: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "default010v1"."phys_events""#)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "failed batch must roll back every row");
}

#[tokio::test]
async fn physical_create_rejects_unknown_column() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();
    create_phys_events_table(&pool).await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "phys_events".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let mut map = serde_json::Map::new();
    map.insert("nope".into(), json!("x"));
    let err = service
        .create(&pool, CreateItemsBody::Single(map))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)), "got {:?}", err);
}

#[test]
fn single_pk_rejects_non_single_column_primary_keys() {
    let int_pk_column = |name: &str| ColumnShape {
        name: name.into(),
        field_type: FieldType::Int,
        data_type: String::new(),
        is_nullable: false,
        is_unique: true,
        is_pk: true,
        has_default: false,
        has_auto_increment: false,
        foreign_key: None,
    };

    let no_pk = TableShape {
        schema: "default010v1".into(),
        name: "no_pk".into(),
        columns: vec![],
        pk: vec![],
        readable: None,
        writable: None,
        collection: None,
    };
    assert!(
        matches!(no_pk.single_pk().unwrap_err(), AppError::BadRequest(_)),
        "empty pk must be rejected"
    );

    let composite = TableShape {
        schema: "default010v1".into(),
        name: "composite".into(),
        columns: vec![int_pk_column("a"), int_pk_column("b")],
        pk: vec!["a".into(), "b".into()],
        readable: None,
        writable: None,
        collection: None,
    };
    assert!(
        matches!(composite.single_pk().unwrap_err(), AppError::BadRequest(_)),
        "composite pk must be rejected"
    );
}

#[tokio::test]
async fn grouped_shape_path_groups_global_table() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();

    sqlx::query(
        r#"INSERT INTO "alcedo"."alcedo_versions" (version_name) VALUES ('v1'), ('v2'), ('v2')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "alcedo_versions".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);

    let response = service
        .read_grouped_for_table(
            &pool,
            TableRef {
                schema: Some("alcedo".into()),
                name: "alcedo_versions".into(),
            },
            GroupedQueryRequest {
                group_by: "version_name".into(),
                filter: None,
                sort: None,
                limit: None,
                offset: None,
            },
            &[],
        )
        .await
        .unwrap();

    assert_eq!(response.total, 2, "two distinct version_name groups");
    assert_eq!(response.groups.len(), 2);

    let counts: std::collections::HashMap<Option<String>, i64> = response
        .groups
        .iter()
        .map(|g| (g.value.clone(), g.count))
        .collect();
    assert_eq!(counts.get(&Some("v1".to_string())), Some(&1));
    assert_eq!(counts.get(&Some("v2".to_string())), Some(&2));

    let v2 = response
        .groups
        .iter()
        .find(|g| g.value.as_deref() == Some("v2"))
        .expect("v2 group should exist");
    assert_eq!(v2.items.len(), 2, "v2 group items should be materialized");
    for item in &v2.items {
        assert_eq!(item.get("version_name"), Some(&json!("v2")));
    }

    let filtered = execute_grouped_for_table(
        &pool,
        &TableShape::resolve(
            &core,
            &ctx,
            TableRef {
                schema: Some("alcedo".into()),
                name: "alcedo_versions".into(),
            },
        )
        .await
        .unwrap(),
        GroupedQueryRequest {
            group_by: "version_name".into(),
            filter: Some(FilterCondition::Rule {
                field: "version_name".into(),
                operator: ComparisonOperator::Eq,
                value: Some(json!("v2")),
            }),
            sort: Some(vec![SortField {
                field: "id".into(),
                order: "desc".into(),
            }]),
            limit: None,
            offset: None,
        },
        &[],
    )
    .await
    .unwrap();

    assert_eq!(filtered.total, 1, "filter should collapse to one group");
    assert_eq!(filtered.groups.len(), 1);
    assert_eq!(filtered.groups[0].value.as_deref(), Some("v2"));
    assert_eq!(filtered.groups[0].count, 2);

    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: Some("alcedo".into()),
            name: "alcedo_versions".into(),
        },
    )
    .await
    .unwrap();
    let page_request = |limit: u64, offset: u64| GroupedQueryRequest {
        group_by: "version_name".into(),
        filter: None,
        sort: None,
        limit: Some(limit),
        offset: Some(offset),
    };

    let first_page = execute_grouped_for_table(&pool, &shape, page_request(1, 0), &[])
        .await
        .unwrap();
    assert_eq!(first_page.total, 2, "total counts every distinct group");
    assert_eq!(first_page.groups.len(), 1, "limit caps returned groups");
    assert_eq!(first_page.groups[0].value.as_deref(), Some("v1"));

    let second_page = execute_grouped_for_table(&pool, &shape, page_request(1, 1), &[])
        .await
        .unwrap();
    assert_eq!(second_page.total, 2);
    assert_eq!(second_page.groups.len(), 1, "offset skips the first group");
    assert_eq!(second_page.groups[0].value.as_deref(), Some("v2"));
}

#[tokio::test]
async fn grouped_shape_path_preserves_null_sentinel() {
    let db = common::TestDb::new().await.unwrap();
    let pool = db.pool().clone();

    sqlx::query(
        r#"CREATE TABLE "default010v1"."general_shape_nullable" (
             "id" serial PRIMARY KEY,
             "status" text
           )"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO "default010v1"."general_shape_nullable" ("status")
           VALUES (NULL), ('open'), ('open')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = ctx();
    TableService::new(&core, &ctx).refresh_schema().await;

    let shape = TableShape::resolve(
        &core,
        &ctx,
        TableRef {
            schema: None,
            name: "general_shape_nullable".into(),
        },
    )
    .await
    .unwrap();
    assert!(shape.collection.is_none());

    let request = || GroupedQueryRequest {
        group_by: "status".into(),
        filter: None,
        sort: None,
        limit: None,
        offset: None,
    };

    let response = execute_grouped_for_table(&pool, &shape, request(), &[])
        .await
        .unwrap();

    assert_eq!(response.total, 2, "NULL counts as its own distinct group");

    let null_group = response
        .groups
        .iter()
        .find(|g| g.value.is_none())
        .expect("NULL group should be preserved via the sentinel");
    assert_eq!(null_group.count, 1);
    assert_eq!(null_group.items.len(), 1);
    assert!(
        null_group.items[0]
            .get("status")
            .map_or(true, |v| v.is_null()),
        "NULL item should round-trip as JSON null"
    );

    let open_group = response
        .groups
        .iter()
        .find(|g| g.value.as_deref() == Some("open"))
        .expect("open group should exist");
    assert_eq!(open_group.count, 2);

    let permissions_err = execute_grouped_for_table(
        &pool,
        &shape,
        request(),
        &[alcedo_db::services::permissions::PolicyPermission {
            id: uuid::Uuid::nil(),
            policy_id: uuid::Uuid::nil(),
            collection_name: "general_shape_nullable".into(),
            action: "read".into(),
            fields: None,
            filter: json!({}),
            field_validation: None,
        }],
    )
    .await
    .unwrap_err();
    assert!(
        matches!(permissions_err, AppError::BadRequest(_)),
        "physical grouped reads must reject permissions, got {:?}",
        permissions_err
    );
}
