use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use regex::Regex;
use serde_json::{Map, json};
use sqlx::Column;
use sqlx::Row;
use sqlx::Transaction;
use sqlx::TypeInfo;
use sqlx::ValueRef;
use sqlx::postgres::PgDatabaseError;
use sqlx::{
    Decode, Error, Pool, Postgres,
    postgres::{PgPoolOptions, PgRow},
};
use tokio::time::Instant;
use uuid::Uuid;

use crate::services::config::Config;
use crate::{AppState, services::errors::AlcedoError};

pub async fn setup_pool(config: &Config) -> Result<Arc<Pool<Postgres>>, Error> {
    let pool = PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .connect(&config.database_url)
        .await?;

    return Ok(Arc::new(pool));
}

pub async fn execute_query(app_state: &AppState, query: String) -> Result<Vec<PgRow>, AlcedoError> {
    let start = Instant::now();

    let rows = sqlx::query(&query)
        .fetch_all(&*app_state.database_pool)
        .await;

    let duration = start.elapsed();

    if app_state.config.database_log_queries {
        println!("Query {} {:.2?}", query, duration);
    }

    process_query_response(rows, &query)
}
pub async fn execute_query_transaction(
    app_state: &AppState,
    ts: &mut Transaction<'_, Postgres>,
    query: &str,
) -> Result<Vec<PgRow>, AlcedoError> {
    let start = Instant::now();

    let rows = sqlx::query(&query).fetch_all(&mut **ts).await;

    let duration = start.elapsed();

    if app_state.config.database_log_queries {
        println!("Query {} {:.2?}", query, duration);
    }

    process_query_response(rows, query)
}

pub fn process_query_response(
    result: Result<Vec<PgRow>, Error>,
    query: &str,
) -> Result<Vec<PgRow>, AlcedoError> {
    match result {
        Ok(rows) => Ok(rows),
        Err(e) => {
            return Err(process_query_error_response(e, query));
        }
    }
}

pub fn process_query_error_response(e: Error, query: &str) -> AlcedoError {
    if let Some(e) = e.as_database_error() {
        let code = e.code();

        if None == code {
            return AlcedoError::SystemError(format!("Query failed: {}", query), 10001);
        }
        let code = code.unwrap();

        if code == "42P01" {
            // Table not found
            return AlcedoError::NotFound(e.message().to_string(), 10002);
        }
        if code == "42601" {
            // Query syntax error
            return AlcedoError::InvalidInput(e.message().to_string(), 10004);
        }
        if code == "42883" {
            return AlcedoError::InvalidInput(
                "Incorrect type provided for field.".to_string(),
                10005,
            );
        }
        if code == "42P07" {
            return AlcedoError::InvalidInput("Collection already exists.".to_string(), 10006);
        }
        if code == "23502" {
            if let Some(e) = e.try_downcast_ref::<PgDatabaseError>() {
                return AlcedoError::InvalidInput(
                    format!(
                        "The field {} has an empty value but is required.",
                        e.column().unwrap()
                    ),
                    10006,
                );
            }
        }
        if code == "23505" {
            if let Some(e) = e.try_downcast_ref::<PgDatabaseError>() {
                let field_name = parse_column_from_string(e.detail().unwrap())
                    .unwrap_or(("unknown".to_string(), "unknown".to_string()));
                return AlcedoError::InvalidInput(
                    format!(
                        "The field {} has the duplicated value {}.",
                        field_name.0, field_name.1
                    ),
                    10007,
                );
            }
        }
        // 23503: FK violation (referenced row missing). 23001: restrict
        // violation (this row is still referenced by another row).
        if code == "23503" || code == "23001" {
            if let Some(e) = e.try_downcast_ref::<PgDatabaseError>() {
                let constraint = e.constraint().unwrap_or("unknown");
                return AlcedoError::InvalidInput(
                    format!(
                        "This record is referenced by another record and cannot be deleted or changed (constraint {}).",
                        constraint
                    ),
                    10008,
                );
            }
        }
    }
    println!("{:?}", e);
    return AlcedoError::Other("Unkown error occured!".to_string(), 10003);
}

/*
    Parsing pg errors with details "Key (id)=(1) already exists." into "id"
*/
fn parse_column_from_string(s: &str) -> Option<(String, String)> {
    let re = Regex::new(r"\((\w+)\)=\(([^)]+)\)").unwrap();

    // Search for the pattern in the string
    if let Some(captures) = re.captures(s) {
        // Retrieve the captured strings from their indices (1 and 2)
        let key = captures.get(1).map(|m| m.as_str().to_string());
        let value = captures.get(2).map(|m| m.as_str().to_string());

        // We use .and_then() to ensure both captures were successful before returning
        if let (Some(k), Some(v)) = (key, value) {
            return Some((k, v));
        }
    }

    // Return None if no pattern is found
    None
}

#[cfg(test)]
mod tests {
    use crate::utils;

    use super::*;
    use sqlx::Row;

    #[tokio::test]
    async fn test_query() {
        let state = utils::test_utils::get_app_state().await;
        let result = execute_query(&state, "select 1 + 1 as result".to_string()).await;

        if result.is_err() {
            panic!("Query not executed");
        }
        let result = result.unwrap();
        let result = result.get(0).unwrap();
        let result: i32 = result.get("result");

        assert_eq!(result, 2);
    }
    #[tokio::test]
    async fn test_query_errors() {
        let state = utils::test_utils::get_app_state().await;
        let result = execute_query(&state, "select 1 + 1 as result".to_string()).await;

        if result.is_err() {
            panic!("Query not executed");
        }
        let result = execute_query(&state, "se".to_string()).await;
        // println!("{:?}",result);
        if let Err(AlcedoError::InvalidInput(_, code)) = result {
            assert_eq!(code, 10004);
        } else {
            panic!("Error: query not failed...");
        }
        let result =
            execute_query(&state, "select * from test_table_non_existing".to_string()).await;

        if let Err(AlcedoError::NotFound(_, code)) = result {
            assert_eq!(code, 10002);
        } else {
            panic!("Error: query not failed...");
        }
    }
}

pub fn pgrow_to_json(row: &PgRow) -> Result<Map<String, serde_json::Value>, sqlx::Error> {
    let mut map = Map::new();

    for (i, column) in row.columns().iter().enumerate() {
        let key = column.name().to_string();
        let raw_value = row.try_get_raw(i)?;
        let json_value;

        // Check for NULL first
        if raw_value.is_null() {
            json_value = serde_json::Value::Null;
        } else {
            // Get the type name from the type info
            let type_name = column.type_info().name();

            json_value = match type_name {
                "UUID" => {
                    let val: Uuid =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val.to_string())
                }
                // Common Integer Types
                "INT2" | "INT4" | "INT8" => {
                    let val: i64 =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val)
                }

                // Common Floating Point Types
                "FLOAT4" | "FLOAT8" => {
                    let val: f64 =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val)
                }

                // Boolean Type
                "BOOL" => {
                    let val: bool =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val)
                }

                // String/Text Types
                "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" => {
                    let val: String =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val)
                }

                // JSON/JSONB Types (can be decoded directly into serde_json::Value)
                "JSON" | "JSONB" => {
                    // This requires the 'json' feature in sqlx
                    let val: serde_json::Value =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    val
                }

                // Date/Time Types
                "DATE" => {
                    let val: NaiveDate =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val.format("%Y-%m-%d").to_string())
                }
                "TIME" => {
                    let val: NaiveTime =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val.format("%H:%M:%S%.f").to_string())
                }
                "TIMESTAMPTZ" => {
                    let val: DateTime<Utc> =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
                }
                "TIMESTAMP" => {
                    let val: NaiveDateTime =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
                }
                "TIMETZ" => {
                    let val: (NaiveTime, i32) =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val.0.format("%H:%M:%S%.f").to_string())
                }
                "INTERVAL" => {
                    let val: String =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val)
                }

                // Fallback for complex types (UUIDs, Decimals, Arrays, etc.)
                // Decodes them as a string to avoid precision loss (e.g., NUMERIC)
                // or complex type dependencies (e.g., chrono, uuid).
                _ => {
                    let val: String =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val)
                }
            };
        }

        map.insert(key, json_value);
    }

    Ok(map)
}
