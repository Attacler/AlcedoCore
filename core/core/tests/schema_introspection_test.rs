use plugin_core::db::schema::get_table_schemas;
use sqlx::PgPool;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

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

async fn create_plugin_schema(pool: &PgPool, schema: &str) -> Result<(), sqlx::Error> {
    let create_schema = format!(r#"CREATE SCHEMA IF NOT EXISTS "{}""#, schema);
    sqlx::query(&create_schema).execute(pool).await?;

    let create_table = format!(
        r#"CREATE TABLE "{}"."items" (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            price NUMERIC(10,2),
            category_id INTEGER,
            created_at TIMESTAMPTZ DEFAULT NOW()
        )"#,
        schema
    );
    sqlx::query(&create_table).execute(pool).await?;

    let create_categories = format!(
        r#"CREATE TABLE "{}"."categories" (
            id SERIAL PRIMARY KEY,
            label TEXT NOT NULL
        )"#,
        schema
    );
    sqlx::query(&create_categories).execute(pool).await?;

    let add_fk = format!(
        r#"ALTER TABLE "{}"."items" ADD CONSTRAINT fk_category
           FOREIGN KEY (category_id) REFERENCES "{}"."categories"(id)"#,
        schema, schema
    );
    sqlx::query(&add_fk).execute(pool).await?;

    let create_migrations = format!(
        r#"CREATE TABLE IF NOT EXISTS "{}"."_sqlx_migrations" (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            installed_on TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            success BOOLEAN NOT NULL,
            checksum BYTEA NOT NULL DEFAULT '\x00',
            execution_time BIGINT NOT NULL DEFAULT 0
        )"#,
        schema
    );
    sqlx::query(&create_migrations).execute(pool).await?;

    Ok(())
}

#[tokio::test]
async fn test_discover_columns() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    create_plugin_schema(test_db.pool(), "plugin_test_discover").await.expect("Failed to create schema");

    let schemas = get_table_schemas(test_db.pool(), "plugin_test_discover")
        .await
        .expect("Failed to get table schemas");

    let items = schemas.iter().find(|t| t.table_name == "items").expect("items table not found");
    assert!(!items.columns.is_empty(), "items should have columns");

    let id_col = items.columns.iter().find(|c| c.column_name == "id").expect("id column not found");
    assert!(!id_col.is_nullable, "id should not be nullable");
    assert_eq!(id_col.ordinal_position, 1);

    let name_col = items.columns.iter().find(|c| c.column_name == "name").expect("name column not found");
    assert!(!name_col.is_nullable, "name should not be nullable");
    assert_eq!(name_col.data_type, "text");
}

#[tokio::test]
async fn test_detect_primary_key() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    create_plugin_schema(test_db.pool(), "plugin_test_pk").await.expect("Failed to create schema");

    let schemas = get_table_schemas(test_db.pool(), "plugin_test_pk")
        .await
        .expect("Failed to get table schemas");

    let items = schemas.iter().find(|t| t.table_name == "items").expect("items table not found");
    let pk = items.primary_key.as_ref().expect("items should have a primary key");
    assert!(pk.columns.contains(&"id".to_string()), "PK should include id column");
}

#[tokio::test]
async fn test_detect_foreign_key() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    create_plugin_schema(test_db.pool(), "plugin_test_fk").await.expect("Failed to create schema");

    let schemas = get_table_schemas(test_db.pool(), "plugin_test_fk")
        .await
        .expect("Failed to get table schemas");

    let items = schemas.iter().find(|t| t.table_name == "items").expect("items table not found");
    let fk = items
        .foreign_keys
        .iter()
        .find(|f| f.column_name == "category_id")
        .expect("category_id FK not found");

    assert_eq!(fk.foreign_table_name, "categories");
    assert_eq!(fk.foreign_column_name, "id");
}

#[tokio::test]
async fn test_excludes_migration_table() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    create_plugin_schema(test_db.pool(), "plugin_test_exclude").await.expect("Failed to create schema");

    let schemas = get_table_schemas(test_db.pool(), "plugin_test_exclude")
        .await
        .expect("Failed to get table schemas");

    let has_migrations = schemas.iter().any(|t| t.table_name == "_sqlx_migrations");
    assert!(!has_migrations, "_sqlx_migrations table should be excluded");
}

#[tokio::test]
async fn test_non_existent_schema_returns_empty() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let schemas = get_table_schemas(test_db.pool(), "plugin_nonexistent")
        .await
        .expect("Non-existent schema should not error");
    assert!(schemas.is_empty(), "Non-existent schema should return empty vec");
}
