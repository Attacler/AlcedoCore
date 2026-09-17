//! Step 0 spike: does `ItemsService`/`Query` resolve 1:M nested fields?
//!
//! Creates `customers` + `contacts` (contacts.customer M:1 -> customers) plus
//! the collection metadata the engine needs for direction detection, then asks
//! the engine for `contacts.first_name` / `contacts.last_name`.
//!
//! Expected: each customer row gains
//!   "contacts": [ { "first_name": ..., "last_name": ... }, ... ]

#[path = "common/mod.rs"]
mod common;

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_db::core_state_for_migrations;
use alcedo_db::services::items::query::Query;
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::tables::TableService;
use plugin_core::config::AppConfig;

const CUSTOMER_ID: &str = "11111111-1111-1111-1111-111111111111";
const CONTACT_A_ID: &str = "22222222-2222-2222-2222-222222222222";
const CONTACT_B_ID: &str = "33333333-3333-3333-3333-333333333333";

#[tokio::test]
async fn itemservice_resolves_one_to_many_fields() {
    let (pool, _container) = common::start_postgres().await.unwrap();

    sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "default010v1""#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        r#"CREATE TABLE "default010v1"."customers" (
             "id" uuid PRIMARY KEY,
             "name" text
           )"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"CREATE TABLE "default010v1"."contacts" (
             "id" uuid PRIMARY KEY,
             "customer" uuid REFERENCES "default010v1"."customers"("id"),
             "first_name" text,
             "last_name" text
           )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(r#"INSERT INTO "default010v1"."customers" ("id","name") VALUES ($1::uuid,$2)"#)
        .bind(CUSTOMER_ID)
        .bind("Acme")
        .execute(&pool)
        .await
        .unwrap();
    for (id, first, last) in [
        (CONTACT_A_ID, "Alice", "Anderson"),
        (CONTACT_B_ID, "Bob", "Brown"),
    ] {
        sqlx::query(
            r#"INSERT INTO "default010v1"."contacts" ("id","customer","first_name","last_name")
               VALUES ($1::uuid,$2::uuid,$3,$4)"#,
        )
        .bind(id)
        .bind(CUSTOMER_ID)
        .bind(first)
        .bind(last)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Collection metadata used by the engine to detect the 1:M direction.
    common::create_meta_tables(&pool).await;
    sqlx::query(
        r#"INSERT INTO "default010v1"."alcedocore_collection_definitions" (name, display_name)
           VALUES ('customers','Customers'), ('contacts','Contacts')"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    common::add_field(&pool, "customers", "id", "uuid", 1, None, None).await;
    common::add_field(&pool, "customers", "name", "text", 2, None, None).await;
    common::add_field(
        &pool,
        "customers",
        "contacts",
        "relationship",
        3,
        Some("contacts"),
        Some("one_to_many"),
    )
    .await;

    common::add_field(&pool, "contacts", "id", "uuid", 1, None, None).await;
    common::add_field(
        &pool,
        "contacts",
        "customer",
        "relationship",
        2,
        Some("customers"),
        Some("many_to_one"),
    )
    .await;
    common::add_field(&pool, "contacts", "first_name", "text", 3, None, None).await;
    common::add_field(&pool, "contacts", "last_name", "text", 4, None, None).await;

    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let ctx = AppContext {
        app_name: "default".to_string(),
        version: "v1".to_string(),
        request_source: RequestSource::FirstMigration,
    };
    TableService::new(&core, &ctx).refresh_schema().await;

    let collection = "customers".to_string();
    let service = ItemsService::new(&core, &ctx, &collection);
    let result = service
        .read_items_by_query(Query {
            fields: vec![
                "name".to_string(),
                "contacts.first_name".to_string(),
                "contacts.last_name".to_string(),
            ],
            ..Default::default()
        })
        .await;

    match &result {
        Ok(rows) => println!("SPIKE OK rows = {:#?}", rows),
        Err(e) => println!("SPIKE ERR = {:?}", e),
    }

    let rows = result.expect("ItemsService failed to resolve 1:M field `contacts.*`");
    assert_eq!(rows.len(), 1, "expected one customer");
    assert_eq!(rows[0].get("name"), Some(&serde_json::json!("Acme")));
    let contacts = rows[0]
        .get("contacts")
        .expect("row is missing `contacts` key");
    let arr = contacts
        .as_array()
        .expect("`contacts` should be a JSON array (1:M expansion)");
    assert_eq!(arr.len(), 2, "expected two contacts for the customer");

    // The base PK is only fetched for grouping and must not leak into output.
    assert!(
        rows[0].get("id").is_none(),
        "base PK `id` should not be included when not requested"
    );

    let names: Vec<&str> = arr
        .iter()
        .filter_map(|c| c.get("first_name").and_then(|v| v.as_str()))
        .collect();
    assert!(names.contains(&"Alice") && names.contains(&"Bob"));
}
