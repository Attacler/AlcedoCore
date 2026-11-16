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
            // Table not found — log full PG message, return generic to client
            tracing::error!("[PG] 42P01: {}", e.message());
            return AlcedoError::NotFound("Resource not found".to_string(), 10002);
        }
        if code == "42601" {
            // Query syntax error — log full PG message, return generic to client
            tracing::error!("[PG] 42601: {}", e.message());
            return AlcedoError::InvalidInput("Invalid query".to_string(), 10004);
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

    fn numeric_bytes(weight: i16, sign: u16, dscale: u16, digits: &[u16]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(digits.len() as u16).to_be_bytes());
        b.extend_from_slice(&weight.to_be_bytes());
        b.extend_from_slice(&sign.to_be_bytes());
        b.extend_from_slice(&dscale.to_be_bytes());
        for d in digits {
            b.extend_from_slice(&d.to_be_bytes());
        }
        b
    }

    #[test]
    fn test_numeric_binary_to_string() {
        // 128 as NUMERIC: ndigits=1, weight=0, sign=pos, dscale=0, digits=[128]
        assert_eq!(
            numeric_binary_to_string(&numeric_bytes(0, 0x0000, 0, &[128])).unwrap(),
            "128"
        );
        assert_eq!(
            numeric_binary_to_string(&numeric_bytes(0, 0x4000, 0, &[42])).unwrap(),
            "-42"
        );
        // 1.50
        assert_eq!(
            numeric_binary_to_string(&numeric_bytes(0, 0x0000, 2, &[1, 5000])).unwrap(),
            "1.50"
        );
        // 0.00128 = 12*10000^-1 + 8000*10000^-2
        assert_eq!(
            numeric_binary_to_string(&numeric_bytes(-1, 0x0000, 5, &[12, 8000])).unwrap(),
            "0.00128"
        );
        // 0.00
        assert_eq!(
            numeric_binary_to_string(&numeric_bytes(0, 0x0000, 2, &[])).unwrap(),
            "0.00"
        );
        // 10000 = 1*10000^1 + 0*10000^0
        assert_eq!(
            numeric_binary_to_string(&numeric_bytes(1, 0x0000, 0, &[1, 0])).unwrap(),
            "10000"
        );
        // 12345678901234567890 (beyond i64)
        assert_eq!(
            numeric_binary_to_string(&numeric_bytes(4, 0x0000, 0, &[1234, 5678, 9012, 3456, 7890]))
                .unwrap(),
            "12345678901234567890"
        );
        // NaN
        assert_eq!(
            numeric_binary_to_string(&numeric_bytes(0, 0xC000, 0, &[])).unwrap(),
            "NaN"
        );
        // truncated payload is an error, not garbage
        assert!(numeric_binary_to_string(&[0, 1, 0, 0]).is_err());
    }

    #[tokio::test]
    async fn test_pgrow_to_json_numeric_returning() {
        let state = utils::test_utils::get_app_state().await;
        let schema = "pool_test_numeric";
        execute_query(&state, format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .await
            .unwrap();
        execute_query(&state, format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        execute_query(
            &state,
            format!("CREATE TABLE {schema}.t (id NUMERIC PRIMARY KEY, val NUMERIC, big NUMERIC)"),
        )
        .await
        .unwrap();

        // Write path: INSERT ... RETURNING returns NUMERIC in binary wire
        // format; it must decode exactly, not fail the String UTF-8 fallback.
        let rows = execute_query(
            &state,
            format!(
                "INSERT INTO {schema}.t (id, val, big) VALUES (128, 1.50, 12345678901234567890) RETURNING id, val, big"
            ),
        )
        .await
        .unwrap();
        let map = pgrow_to_json(rows.get(0).unwrap()).unwrap();
        assert_eq!(map.get("id"), Some(&json!(128)));
        assert_eq!(map.get("val"), Some(&json!("1.50")));
        assert_eq!(map.get("big"), Some(&json!(12345678901234567890u64)));

        execute_query(&state, format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }

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

                // Common Floating Point Types (FLOAT4 is 4 bytes on the wire,
                // so it must be decoded as f32 — decoding it as f64 panics in
                // byteorder)
                "FLOAT4" => {
                    let val: f32 =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val as f64)
                }
                "FLOAT8" => {
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

                // Array types (e.g. `uuid[]` file fields). sqlx names them
                // `_uuid`/`UUID[]` depending on the source.
                "_UUID" | "UUID[]" => {
                    let val: Vec<Uuid> =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val.iter().map(|id| id.to_string()).collect::<Vec<_>>())
                }
                "_TEXT" | "TEXT[]" | "_VARCHAR" | "VARCHAR[]" => {
                    let val: Vec<String> =
                        Decode::<'_, Postgres>::decode(raw_value).map_err(sqlx::Error::Decode)?;
                    json!(val)
                }

                // NUMERIC arrives in the binary wire format (sqlx requests
                // binary for every result column) and is not valid UTF-8, so
                // the String fallback below cannot decode it — e.g. an
                // INSERT ... RETURNING on a numeric primary key. Decode the
                // base-10000 digit encoding to the exact text form; emit a
                // JSON number when the value is an exact integer.
                "NUMERIC" => {
                    let bytes = raw_value.as_bytes().map_err(sqlx::Error::Decode)?;
                    let text = if raw_value.format() == sqlx::postgres::PgValueFormat::Text {
                        std::str::from_utf8(bytes)
                            .map_err(|e| sqlx::Error::Decode(e.into()))?
                            .to_string()
                    } else {
                        numeric_binary_to_string(bytes).map_err(sqlx::Error::Decode)?
                    };
                    match text.parse::<i64>() {
                        Ok(n) => json!(n),
                        Err(_) => match text.parse::<u64>() {
                            Ok(n) => json!(n),
                            Err(_) => json!(text),
                        },
                    }
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

/// Decodes the binary wire format of a Postgres NUMERIC to its exact text
/// form: u16 ndigits, i16 weight, u16 sign, u16 dscale, then `ndigits` u16
/// digits in base 10000 (most significant first), where the value is
/// `sum(digits[i] * 10000^(weight - i))`.
fn numeric_binary_to_string(bytes: &[u8]) -> Result<String, sqlx::error::BoxDynError> {
    if bytes.len() < 8 {
        return Err(format!("invalid NUMERIC payload: {} bytes", bytes.len()).into());
    }
    let ndigits = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
    let weight = i16::from_be_bytes([bytes[2], bytes[3]]);
    let sign = u16::from_be_bytes([bytes[4], bytes[5]]);
    let dscale = u16::from_be_bytes([bytes[6], bytes[7]]) as usize;
    if bytes.len() < 8 + ndigits * 2 {
        return Err("truncated NUMERIC digits".into());
    }
    let digits: Vec<u16> = (0..ndigits)
        .map(|i| u16::from_be_bytes([bytes[8 + i * 2], bytes[9 + i * 2]]))
        .collect();

    match sign {
        0xC000 => return Ok("NaN".to_string()),
        0xD000 => return Ok("Infinity".to_string()),
        0xF000 => return Ok("-Infinity".to_string()),
        0x0000 | 0x4000 => {}
        other => return Err(format!("invalid NUMERIC sign: {other:#06X}").into()),
    }

    let mut out = String::new();
    if sign == 0x4000 {
        out.push('-');
    }

    // Integer part: base-10000 groups 0..=weight (missing groups are zero).
    if weight >= 0 {
        for i in 0..=(weight as usize) {
            let d = digits.get(i).copied().unwrap_or(0);
            if i == 0 {
                out.push_str(&d.to_string());
            } else {
                out.push_str(&format!("{d:04}"));
            }
        }
    } else {
        out.push('0');
    }

    // Fraction: groups after `weight`, with leading zero groups when
    // weight < 0, padded/truncated to the display scale.
    if dscale > 0 {
        let mut frac = String::new();
        if weight < 0 {
            for _ in 0..(-(weight + 1) as usize) {
                frac.push_str("0000");
            }
        }
        for d in digits.iter().skip((weight + 1).max(0) as usize) {
            frac.push_str(&format!("{d:04}"));
        }
        if frac.len() < dscale {
            frac.push_str(&"0".repeat(dscale - frac.len()));
        } else {
            frac.truncate(dscale);
        }
        out.push('.');
        out.push_str(&frac);
    }

    Ok(out)
}
