use serde::{Deserialize, Serialize};

use crate::db::query_builder::{execute_query, QueryRequest, QueryResponse};
use crate::db::schema::get_table_schemas;
use crate::error::AppError;
use crate::db::Pool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRequest {
    pub items: Vec<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRequest {
    pub filter: serde_json::Value,
    pub update: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRequest {
    pub filter: Option<serde_json::Value>,
    pub pk_values: Option<Vec<serde_json::Value>>,
}

async fn resolve_schema(pool: &Pool, slug: &str, table_name: Option<&str>) -> Result<(String, String, Vec<String>), AppError> {
    let schema_name = crate::db::plugin_migrations::plugin_schema_name(slug);
    let schemas = get_table_schemas(pool, &schema_name).await?;

    if schemas.is_empty() {
        return Err(AppError::NotFound(format!(
            "No tables found for plugin '{}'",
            slug
        )));
    }

    let table = if let Some(name) = table_name {
        schemas
            .into_iter()
            .find(|t| t.table_name == name)
            .ok_or_else(|| AppError::NotFound(format!("Table '{}' not found for plugin '{}'", name, slug)))?
    } else {
        schemas
            .into_iter()
            .find(|t| t.table_name != "_sqlx_migrations")
            .ok_or_else(|| AppError::NotFound(format!("No user tables for plugin '{}'", slug)))?
    };

    let col_names: Vec<String> = table.columns.iter().map(|c| c.column_name.clone()).collect();
    Ok((schema_name, table.table_name, col_names))
}

fn validate_columns(
    keys: &[String],
    valid_columns: &[String],
) -> Result<(), AppError> {
    for key in keys {
        if !valid_columns.contains(key) {
            return Err(AppError::BadRequest(format!(
                "Invalid column: '{}'. Valid columns: [{}]",
                key,
                valid_columns.join(", ")
            )));
        }
    }
    Ok(())
}

pub async fn query_items(
    pool: &Pool,
    slug: &str,
    request: QueryRequest,
) -> Result<QueryResponse, AppError> {
    let (schema_name, _table_name, _columns) = resolve_schema(pool, slug, None).await?;
    let schemas = get_table_schemas(pool, &schema_name).await?;
    let table_schema = schemas.into_iter().find(|t| t.table_name != "_sqlx_migrations").unwrap();
    execute_query(pool, &schema_name, &table_schema, &request).await
}

pub async fn create_items(
    pool: &Pool,
    slug: &str,
    request: CreateRequest,
) -> Result<Vec<serde_json::Value>, AppError> {
    let (schema_name, table_name, valid_columns) = resolve_schema(pool, slug, None).await?;
    let table_name = table_name.clone();

    if request.items.is_empty() {
        return Err(AppError::BadRequest("No items provided".to_string()));
    }

    for item in &request.items {
        let keys: Vec<String> = item.keys().cloned().collect();
        validate_columns(&keys, &valid_columns)?;
    }

    let mut tx = pool.begin().await?;
    let mut results = Vec::new();

    for item in &request.items {
        let cols: Vec<&String> = item.keys().collect();
        let quoted: Vec<String> = cols.iter().map(|c| format!("\"{}\"", c)).collect();
        let ph: Vec<String> = (1..=cols.len()).map(|i| format!("${}", i)).collect();

        let sql = format!(
            "INSERT INTO \"{}\".\"{}\" ({}) VALUES ({}) RETURNING row_to_json(\"{}\".\"{}\".*)",
            schema_name, table_name, quoted.join(", "), ph.join(", "),
            schema_name, table_name
        );

        let mut q = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
        for col in cols {
            let val = item.get(col.as_str()).cloned().unwrap_or(serde_json::Value::Null);
            q = crate::bind_json_value_owned!(q, val);
        }

        let (row,): (serde_json::Value,) = q.fetch_one(&mut *tx).await.map_err(|e| {
            AppError::DatabaseError { details: format!("Insert failed: {}", e) }
        })?;
        results.push(row);
    }

    tx.commit().await?;
    Ok(results)
}

pub async fn update_items(
    pool: &Pool,
    slug: &str,
    request: UpdateRequest,
) -> Result<u64, AppError> {
    let (schema_name, table_name, valid_columns) = resolve_schema(pool, slug, None).await?;

    let update_keys: Vec<String> = request.update.keys().cloned().collect();
    validate_columns(&update_keys, &valid_columns)?;

    let set_clauses: Vec<String> = request.update.keys().enumerate()
        .map(|(i, k)| format!("\"{}\" = ${}", k, i + 1))
        .collect();
    let update_vals: Vec<serde_json::Value> = request.update.values().cloned().collect();

    let (filter_clause, filter_vals) = if !request.filter.is_null() {
        if let Some(obj) = request.filter.as_object() {
            let offset = update_vals.len();
            let mut conds: Vec<String> = Vec::new();
            let mut vals: Vec<serde_json::Value> = Vec::new();
            for (i, k) in obj.keys().enumerate() {
                conds.push(format!("\"{}\" = ${}", k, offset + i + 1));
                vals.push(obj[k].clone());
            }
            (conds.join(" AND "), vals)
        } else {
            ("TRUE".to_string(), vec![])
        }
    } else {
        ("TRUE".to_string(), vec![])
    };

    let sql = format!(
        "UPDATE \"{}\".\"{}\" SET {} WHERE {}",
        schema_name, table_name, set_clauses.join(", "), filter_clause
    );

    let mut q = sqlx::query(&sql);
    for val in &update_vals {
        q = crate::bind_json_value!(q, val);
    }
    for val in &filter_vals {
        q = crate::bind_json_value!(q, val);
    }

    let result = q.execute(pool).await.map_err(|e| AppError::DatabaseError {
        details: format!("Update failed: {}", e),
    })?;
    Ok(result.rows_affected())
}

pub async fn delete_items(
    pool: &Pool,
    slug: &str,
    request: DeleteRequest,
) -> Result<(u64, Vec<serde_json::Value>), AppError> {
    let (schema_name, table_name, _valid_columns) = resolve_schema(pool, slug, None).await?;

    let pks = ["id"];

    let (where_clause, vals): (String, Vec<serde_json::Value>) =
        if let Some(ref pk_values) = request.pk_values {
            if pk_values.is_empty() {
                return Err(AppError::BadRequest("No pk_values provided".to_string()));
            }
            let phs: Vec<String> = (1..=pk_values.len()).map(|i| format!("${}", i)).collect();
            (format!("\"{}\" IN ({})", pks[0], phs.join(", ")), pk_values.clone())
        } else if let Some(ref filter) = request.filter {
            if let Some(obj) = filter.as_object() {
                let conds: Vec<String> = obj.keys().enumerate()
                    .map(|(i, k)| format!("\"{}\" = ${}", k, i + 1))
                    .collect();
                let vals: Vec<serde_json::Value> = obj.values().cloned().collect();
                (conds.join(" AND "), vals)
            } else {
                return Err(AppError::BadRequest("Filter must be a JSON object".to_string()));
            }
        } else {
            return Err(AppError::BadRequest("filter or pk_values required".to_string()));
        };

    let table_ref = format!("\"{}\".\"{}\"", schema_name, table_name);
    let sql = format!(
        "DELETE FROM {} WHERE {} RETURNING row_to_json({}.*) AS deleted_item",
        table_ref, where_clause, table_ref
    );

    let mut q = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
    for val in &vals {
        q = crate::bind_json_value!(q, val);
    }

    let rows: Vec<(serde_json::Value,)> = q.fetch_all(pool).await.map_err(|e| AppError::DatabaseError {
        details: format!("Delete failed: {}", e),
    })?;
    let count = rows.len() as u64;
    let items: Vec<serde_json::Value> = rows.into_iter().map(|(v,)| v).collect();
    Ok((count, items))
}
