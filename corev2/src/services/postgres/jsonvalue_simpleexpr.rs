use sea_query::{Expr, SimpleExpr};
use serde_json::Value;

use crate::services::{items::query::TableJoin, postgres::inspector::DatabaseSchema};

pub fn parse_value(
    schema: &DatabaseSchema,
    joins: &Vec<TableJoin>,
    table_id: &str,
    field: &str,
    value: Value,
) -> Option<SimpleExpr> {
    let actual_table = joins
        .iter()
        .find(|f| f.id == table_id)
        .map(|join| &join.target_table)
        .map_or(table_id, |v| v);

    let column = schema
        .columns
        .iter()
        .find(|c| c.table == *actual_table && c.name == field)?;

    let sea_value = match column.data_type.as_str() {
        // Consolidated numeric types
        "bigint" | "bigserial" | "int" | "int4" | "serial4" | "int2" | "serial2" | "integer"
        | "smallint" | "float" | "float8" | "float4" | "real" | "double precision" => {
            parse_number(&value, &column.data_type)?
        }

        // Decimal types - using f64 for better precision
        "decimal" | "money" => parse_decimal(&value)?,

        "boolean" => {
            let b = value.as_bool().or_else(|| {
                value
                    .as_str()
                    .and_then(|s| match s.to_lowercase().as_str() {
                        "true" | "t" | "1" => Some(true),
                        "false" | "f" | "0" => Some(false),
                        _ => None,
                    })
            })?;
            sea_query::Value::Bool(Some(b))
        }

        // Text types - only accept strings or explicit conversions
        "text" | "varchar" | "character varying" | "character" | "macaddr" | "macaddr8"
        | "cidr" | "inet" | "bit" | "varbit" | "uuid" => {
            let s = match value {
                Value::String(s) => s,
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => return None, // Don't convert objects/arrays to strings
            };
            sea_query::Value::String(Some(Box::new(s)))
        }

        // Date/time types - string only
        "date" | "time" | "timetz" | "timestamp" | "timestamptz" | "interval" => {
            let s = value.as_str()?.to_uppercase();
            match s.as_str() {
                "CURRENT_DATE" => return Some(Expr::cust("CURRENT_DATE")),
                "CURRENT_TIMESTAMP" => return Some(Expr::cust("CURRENT_TIMESTAMP")),
                "NOW()" => return Some(Expr::cust("NOW()")),
                _ => sea_query::Value::String(Some(Box::new(value.as_str()?.to_string()))),
            }
        }

        // JSON types
        "json" | "jsonb" => {
            let json_value = match value {
                Value::String(s) => s.parse::<serde_json::Value>().ok()?,
                v => v,
            };
            sea_query::Value::Json(Some(Box::new(json_value)))
        }

        // Geometric types - string only
        "box" | "circle" | "line" | "lseg" | "path" | "point" | "polygon" => {
            let s = value.as_str()?.to_string();
            sea_query::Value::String(Some(Box::new(s)))
        }

        // Unknown types - return None to make errors visible
        _ => return None,
    };

    Some(Expr::value(sea_value))
}

// Helper function for standard numeric types
fn parse_number(v: &Value, data_type: &str) -> Option<sea_query::Value> {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return match data_type {
                    "bigint" | "bigserial" => Some(sea_query::Value::BigInt(Some(i))),
                    "int" | "int4" | "serial4" | "integer" => {
                        Some(sea_query::Value::Int(Some(i as i32)))
                    }
                    "int2" | "smallint" | "serial2" => {
                        Some(sea_query::Value::SmallInt(Some(i as i16)))
                    }
                    _ => None,
                };
            }
            if let Some(f) = n.as_f64() {
                return match data_type {
                    "float" | "float8" | "double precision" => {
                        Some(sea_query::Value::Double(Some(f)))
                    }
                    "float4" | "real" => Some(sea_query::Value::Float(Some(f as f32))),
                    _ => None,
                };
            }
            // Fallback for very large numbers
            Some(sea_query::Value::Double(Some(n.to_string().parse().ok()?)))
        }
        Value::String(s) => match data_type {
            "bigint" | "bigserial" => s
                .parse::<i64>()
                .ok()
                .map(|i| sea_query::Value::BigInt(Some(i))),
            "int" | "int4" | "serial4" | "integer" => s
                .parse::<i32>()
                .ok()
                .map(|i| sea_query::Value::Int(Some(i))),
            "int2" | "smallint" | "serial2" => s
                .parse::<i16>()
                .ok()
                .map(|i| sea_query::Value::SmallInt(Some(i))),
            "float" | "float8" | "double precision" => s
                .parse::<f64>()
                .ok()
                .map(|f| sea_query::Value::Double(Some(f))),
            "float4" | "real" => s
                .parse::<f32>()
                .ok()
                .map(|f| sea_query::Value::Float(Some(f))),
            _ => None,
        },
        _ => None,
    }
}

// Separate helper for decimal/money types with better precision
fn parse_decimal(v: &Value) -> Option<sea_query::Value> {
    match v {
        Value::Number(n) => {
            let f = n.as_f64()?;
            Some(sea_query::Value::Double(Some(f)))
        }
        Value::String(s) => {
            let f = s.parse::<f64>().ok()?;
            Some(sea_query::Value::Double(Some(f)))
        }
        _ => None,
    }
}
