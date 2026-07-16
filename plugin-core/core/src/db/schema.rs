use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::db::Pool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub column_name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub column_default: Option<String>,
    pub ordinal_position: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimaryKey {
    pub constraint_name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKey {
    pub constraint_name: String,
    pub column_name: String,
    pub foreign_table_schema: String,
    pub foreign_table_name: String,
    pub foreign_column_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub table_name: String,
    pub columns: Vec<ColumnSchema>,
    pub primary_key: Option<PrimaryKey>,
    pub foreign_keys: Vec<ForeignKey>,
}

pub async fn get_table_schemas(pool: &Pool, schema: &str) -> Result<Vec<TableSchema>, AppError> {
    let table_names: Vec<(String,)> = sqlx::query_as(
        r#"SELECT table_name::text FROM information_schema.tables
           WHERE table_schema = $1 AND table_type = 'BASE TABLE'
           AND table_name != '_sqlx_migrations'
           ORDER BY table_name"#,
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    let mut tables = Vec::new();
    for (table_name,) in table_names {
        let columns = get_columns(pool, schema, &table_name).await?;
        let primary_key = get_primary_key(pool, schema, &table_name).await?;
        let foreign_keys = get_foreign_keys(pool, schema, &table_name).await?;

        tables.push(TableSchema {
            table_name,
            columns,
            primary_key,
            foreign_keys,
        });
    }

    Ok(tables)
}

async fn get_columns(
    pool: &Pool,
    schema: &str,
    table_name: &str,
) -> Result<Vec<ColumnSchema>, AppError> {
    let rows: Vec<(String, String, bool, Option<String>, i32)> = sqlx::query_as(
        r#"SELECT column_name::text,
                  udt_name::text,
                  is_nullable::text = 'YES' AS is_nullable,
                  column_default::text,
                  ordinal_position::int
           FROM information_schema.columns
           WHERE table_schema = $1 AND table_name = $2
           ORDER BY ordinal_position"#,
    )
    .bind(schema)
    .bind(table_name)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(column_name, data_type, is_nullable, column_default, ordinal_position)| {
            ColumnSchema {
                column_name,
                data_type,
                is_nullable,
                column_default,
                ordinal_position,
            }
        })
        .collect())
}

async fn get_primary_key(
    pool: &Pool,
    schema: &str,
    table_name: &str,
) -> Result<Option<PrimaryKey>, AppError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT tc.constraint_name::text,
                  kcu.column_name::text
           FROM information_schema.table_constraints tc
           JOIN information_schema.key_column_usage kcu
             ON tc.constraint_name = kcu.constraint_name
            AND tc.table_schema = kcu.constraint_schema
           WHERE tc.table_schema = $1
             AND tc.table_name = $2
             AND tc.constraint_type = 'PRIMARY KEY'
           ORDER BY kcu.ordinal_position"#,
    )
    .bind(schema)
    .bind(table_name)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let constraint_name = rows[0].0.clone();
    let columns: Vec<String> = rows.into_iter().map(|(_, col)| col).collect();

    Ok(Some(PrimaryKey {
        constraint_name,
        columns,
    }))
}

async fn get_foreign_keys(
    pool: &Pool,
    schema: &str,
    table_name: &str,
) -> Result<Vec<ForeignKey>, AppError> {
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        r#"SELECT rc.constraint_name::text,
                  kcu.column_name::text,
                  ccu.table_schema::text,
                  ccu.table_name::text,
                  ccu.column_name::text AS foreign_column_name
           FROM information_schema.referential_constraints rc
           JOIN information_schema.key_column_usage kcu
             ON rc.constraint_name = kcu.constraint_name
            AND rc.constraint_schema = kcu.constraint_schema
           JOIN information_schema.constraint_column_usage ccu
             ON rc.unique_constraint_name = ccu.constraint_name
            AND rc.constraint_schema = ccu.constraint_schema
           WHERE kcu.table_schema = $1
             AND kcu.table_name = $2"#,
    )
    .bind(schema)
    .bind(table_name)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(constraint_name, column_name, foreign_table_schema, foreign_table_name, foreign_column_name)| ForeignKey {
                constraint_name,
                column_name,
                foreign_table_schema,
                foreign_table_name,
                foreign_column_name,
            },
        )
        .collect())
}
