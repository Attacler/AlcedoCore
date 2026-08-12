//! ## Phase 77: E2E Tests for v2.7 Advanced Relations
//!
//! Covers: nested field selection, relational CRUD, display field customization,
//! relational sections, and parent field inlining.

use serde_json::json;
use sqlx::PgPool;

#[path = "common/mod.rs"]
mod common;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

use plugin_core::db::collections::{FieldDefinition, FieldType};
use plugin_core::services::collection_builder::CollectionBuilder;

async fn create_coll(pool: &PgPool, name: &str, fields: Vec<FieldDefinition>) {
    let sql = CollectionBuilder::build_create_table_stmt(name, &fields).unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(&sql).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO collection_definitions (name, display_name, display_options) VALUES ($1, $2, '{}')"
    ).bind(name).bind(name).execute(&mut *tx).await.unwrap();
    plugin_core::db::fields::replace_fields_in_tx(&mut tx, name, &fields).await.unwrap();
    tx.commit().await.unwrap();
}

// ---------------------------------------------------------------------------
// TEST-02: Nested field selection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_nested_field_selection() {
    let td = common::TestDb::new().await.unwrap();
    let p = td.pool();

    create_coll(p, "e2e_authors", vec![
        FieldDefinition { name: "name".into(), field_type: FieldType::String, required: true, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
    ]).await;
    create_coll(p, "e2e_articles", vec![
        FieldDefinition { name: "title".into(), field_type: FieldType::String, required: true, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
        FieldDefinition { name: "author_id".into(), field_type: FieldType::Relationship, required: false, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: Some("e2e_authors".into()), relationship_type: Some("many_to_one".into()), display_field: None, inline_parent_fields: None },
    ]).await;

    // Create test data via raw SQL
    sqlx::query("INSERT INTO e2e_authors (name) VALUES ('Alice') RETURNING id")
        .execute(p).await.unwrap();
    let (author_id,): (uuid::Uuid,) = sqlx::query_as("SELECT id FROM e2e_authors LIMIT 1")
        .fetch_one(p).await.unwrap();
    sqlx::query("INSERT INTO e2e_articles (title, author_id) VALUES ('Art 1', $1)")
        .bind(author_id).execute(p).await.unwrap();

    // Resolve nested field path
    use plugin_core::db::field_resolver::{self, FieldResolverOptions};
    let all = plugin_core::db::collections::list_collections(p).await.unwrap();

    let frags = field_resolver::resolve_nested_fields(
        &["author_id.name".into()], "e2e_articles", &all,
        &mut FieldResolverOptions { depth_limit: 5, backlink: true, visited: Default::default() },
    ).unwrap();
    assert_eq!(frags[0].alias, "author_id", "Alias should match");

    // Wildcard expansion
    let frags2 = field_resolver::resolve_nested_fields(
        &["*.*".into()], "e2e_articles", &all,
        &mut FieldResolverOptions { depth_limit: 5, backlink: true, visited: Default::default() },
    ).unwrap();
    assert!(frags2.iter().any(|f| f.alias == "author_id"), "Wildcard should resolve author_id");

    // Depth limit
    let r = field_resolver::resolve_nested_fields(
        &["a.b.c.d.e.f".into()], "e2e_articles", &all,
        &mut FieldResolverOptions { depth_limit: 2, backlink: true, visited: Default::default() },
    );
    assert!(r.is_err(), "Depth limit exceeded should error");

    println!("TEST-02 passed: nested field selection");
}

// ---------------------------------------------------------------------------
// TEST-03: Relational CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_relational_crud() {
    use plugin_core::db::relational_crud;
    use plugin_core::db::collections as db_colls;

    let td = common::TestDb::new().await.unwrap();
    let p = td.pool();

    create_coll(p, "e2e_orgs", vec![
        FieldDefinition { name: "name".into(), field_type: FieldType::String, required: true, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
    ]).await;
    create_coll(p, "e2e_contacts", vec![
        FieldDefinition { name: "name".into(), field_type: FieldType::String, required: true, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
        FieldDefinition { name: "org_id".into(), field_type: FieldType::Relationship, required: false, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: Some("e2e_orgs".into()), relationship_type: Some("many_to_one".into()), display_field: None, inline_parent_fields: None },
    ]).await;

    // Inline M:1 create via relational_crud
    let coll = db_colls::get_collection(p, "e2e_contacts").await.unwrap();
    let all_collections = db_colls::list_collections(p).await.unwrap();
    let mut items = vec![json!({"name": "Bob", "org_id": {"name": "Acme"}}).as_object().unwrap().clone()];
    let mut tx = p.begin().await.unwrap();
    relational_crud::process_create_body_for_relational(&mut tx, &coll, &mut items, &all_collections).await.unwrap();
    tx.commit().await.unwrap();
    assert!(items[0]["org_id"].as_str().is_some(), "org_id should be UUID");

    // Create the contact via items API
    use plugin_core::db::collection_items::CreateItemsBody;
    let results = plugin_core::db::collection_items::create_items(
        p, "e2e_contacts", CreateItemsBody::Single(items[0].clone())
    ).await.unwrap();
    assert_eq!(results.len(), 1);

    println!("TEST-03 passed: relational CRUD");
}

// ---------------------------------------------------------------------------
// TEST-04: Display field customization
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_display_field() {
    use plugin_core::db::collections as db_colls;
    use plugin_core::db::collection_items::{self, CollectionItemsQuery};

    let td = common::TestDb::new().await.unwrap();
    let p = td.pool();

    create_coll(p, "e2e_depts", vec![
        FieldDefinition { name: "name".into(), field_type: FieldType::String, required: true, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
    ]).await;
    create_coll(p, "e2e_emps", vec![
        FieldDefinition { name: "name".into(), field_type: FieldType::String, required: true, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
        FieldDefinition { name: "dept_id".into(), field_type: FieldType::Relationship, required: false, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: Some("e2e_depts".into()), relationship_type: Some("many_to_one".into()), display_field: Some("name".into()), inline_parent_fields: None },
    ]).await;

    // Create test data
    sqlx::query("INSERT INTO e2e_depts (name) VALUES ('Engineering')")
        .execute(p).await.unwrap();
    let (dept_id,): (uuid::Uuid,) = sqlx::query_as("SELECT id FROM e2e_depts LIMIT 1")
        .fetch_one(p).await.unwrap();
    sqlx::query("INSERT INTO e2e_emps (name, dept_id) VALUES ('Charlie', $1)")
        .bind(dept_id).execute(p).await.unwrap();

    // Verify display_field in collection definition
    let coll = db_colls::get_collection(p, "e2e_emps").await.unwrap();
    let f = coll.fields.iter().find(|f| f.name == "dept_id").unwrap();
    assert_eq!(f.display_field.as_deref(), Some("name"));

    // Query items and check display values
    let items = collection_items::query_items(p, "e2e_emps", CollectionItemsQuery {
        limit: Some(100), offset: Some(0), filter: None, sort_field: None, sort_order: None,
    }).await.unwrap();
    assert!(!items.is_empty(), "Should have items");
    let has_dv = items.iter().any(|item| item.get("dept_id__display_value").is_some());
    assert!(has_dv, "Display values should be present in results");

    println!("TEST-04 passed: display field");
}

// ---------------------------------------------------------------------------
// TEST-05: Relational sections
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_relational_sections() {
    let td = common::TestDb::new().await.unwrap();
    let p = td.pool();

    sqlx::query(
        "INSERT INTO collection_sections (collection_name, name, relation_field, view_type, item_limit, ordinal_position) VALUES ($1, $2, $3, $4, $5, $6)"
    ).bind("e2e_test_coll").bind("Test Section").bind("rel_field").bind("table").bind(25).bind(1)
    .execute(p).await.unwrap();

    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT name, relation_field, view_type FROM collection_sections WHERE collection_name = $1"
    ).bind("e2e_test_coll").fetch_all(p).await.unwrap();

    assert_eq!(rows.len(), 1, "Should have 1 section");
    assert_eq!(rows[0].0, "Test Section", "Name should match");

    println!("TEST-05 passed: relational sections");
}

// ---------------------------------------------------------------------------
// TEST-06: Parent field inlining
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_parent_field_inlining() {
    use plugin_core::db::collections as db_colls;

    let td = common::TestDb::new().await.unwrap();
    let p = td.pool();

    create_coll(p, "e2e_companies", vec![
        FieldDefinition { name: "name".into(), field_type: FieldType::String, required: true, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
        FieldDefinition { name: "website".into(), field_type: FieldType::String, required: false, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
    ]).await;
    create_coll(p, "e2e_staff", vec![
        FieldDefinition { name: "name".into(), field_type: FieldType::String, required: true, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: None, relationship_type: None, display_field: None, inline_parent_fields: None },
        FieldDefinition { name: "company_id".into(), field_type: FieldType::Relationship, required: false, unique: false, default: None, display_name: None, display_type: None, options: None, is_system: false, hidden: false, full_width: false, related_collection: Some("e2e_companies".into()), relationship_type: Some("many_to_one".into()), display_field: None, inline_parent_fields: Some(vec!["name".into(), "website".into()]) },
    ]).await;

    // Verify inline_parent_fields in collection definition
    let coll = db_colls::get_collection(p, "e2e_staff").await.unwrap();
    let f = coll.fields.iter().find(|f| f.name == "company_id").unwrap();
    assert!(f.inline_parent_fields.is_some(), "inline_parent_fields should be Some");
    let ipf = f.inline_parent_fields.as_ref().unwrap();
    assert!(ipf.contains(&"name".into()), "Should contain name");
    assert!(ipf.contains(&"website".into()), "Should contain website");

    // Create test data and verify augment_items_with_inline_parents
    sqlx::query("INSERT INTO e2e_companies (name, website) VALUES ('BigCorp', 'https://bigcorp.com')")
        .execute(p).await.unwrap();
    let (co_id,): (uuid::Uuid,) = sqlx::query_as("SELECT id FROM e2e_companies LIMIT 1")
        .fetch_one(p).await.unwrap();
    sqlx::query("INSERT INTO e2e_staff (name, company_id) VALUES ('Diana', $1)")
        .bind(co_id).execute(p).await.unwrap();

    let items = plugin_core::db::collection_items::query_items(p, "e2e_staff",
        plugin_core::db::collection_items::CollectionItemsQuery {
            limit: Some(100), offset: Some(0), filter: None, sort_field: None, sort_order: None,
        }
    ).await.unwrap();
    assert!(!items.is_empty(), "Should have items");

    let augmented = plugin_core::db::collection_items::augment_items_with_inline_parents(
        p, &items, &coll, "e2e_staff"
    ).await.unwrap();
    assert_eq!(augmented.len(), items.len(), "Should preserve count");

    println!("TEST-06 passed: parent field inlining");
}
