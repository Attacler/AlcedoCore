use sea_query::{Expr, SimpleExpr};
use serde_json::Value;

use crate::services::{items::query::TableJoin, postgres::inspector::DatabaseSchema};

pub fn parse_value(
    schema: &DatabaseSchema,
    schema_name: &str,
    joins: &Vec<TableJoin>,
    table_id: &str,
    field: &str,
    value: Value,
) -> Option<SimpleExpr> {
    let (actual_table, actual_schema) = match joins.iter().find(|f| f.id == table_id) {
        Some(join) => (join.target_table.as_str(), join.target_schema.as_str()),
        None => (table_id, schema_name),
    };

    let column = schema
        .columns
        .iter()
        .find(|c| c.table == actual_table && c.name == field && c.schema == actual_schema)?;

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
        "date"
        | "time"
        | "timetz"
        | "timestamp"
        | "timestamptz"
        | "interval"
        | "timestamp without time zone"
        | "timestamp with time zone"
        | "time without time zone"
        | "time with time zone" => {
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
                Value::String(s) => s.parse::<serde_json::Value>().unwrap_or(Value::String(s)),
                v => v,
            };
            sea_query::Value::Json(Some(Box::new(json_value)))
        }

        // Geometric types - string only
        "box" | "circle" | "line" | "lseg" | "path" | "point" | "polygon" => {
            let s = value.as_str()?.to_string();
            sea_query::Value::String(Some(Box::new(s)))
        }

        // Array types (e.g. `uuid[]` for file fields). `data_type` is just
        // "ARRAY"; the element type comes from `udt_name`. Only `uuid[]` is
        // supported (the type produced by the `file` field), and each element
        // is validated so the inlined literal can't be abused.
        "ARRAY" => {
            let element = column.udt_name.trim_start_matches('_');
            if element != "uuid" {
                return None;
            }
            let items = value.as_array()?;
            if items.is_empty() {
                return Some(Expr::cust("ARRAY[]::uuid[]"));
            }
            let mut parsed: Vec<uuid::Uuid> = Vec::with_capacity(items.len());
            for item in items {
                let raw = item.as_str()?;
                parsed.push(uuid::Uuid::parse_str(raw).ok()?);
            }
            let list = parsed
                .iter()
                .map(|id| format!("'{}'::uuid", id))
                .collect::<Vec<_>>()
                .join(", ");
            return Some(Expr::cust(format!("ARRAY[{}]", list)));
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

#[cfg(test)]
mod tests {
    use super::parse_value;
    use crate::services::postgres::inspector::{Column, DatabaseSchema};
    use sea_query::SimpleExpr;
    use serde_json::json;

    fn schema_with_uuid_array() -> DatabaseSchema {
        DatabaseSchema {
            columns: vec![Column {
                schema: "shop010production".to_string(),
                table: "products".to_string(),
                name: "photo".to_string(),
                data_type: "ARRAY".to_string(),
                udt_name: "_uuid".to_string(),
                default_value: None,
                max_length: None,
                numeric_precision: None,
                numeric_scale: None,
                is_nullable: true,
                is_unique: false,
                is_indexed: false,
                is_primary_key: false,
                generated: false,
                generation_expression: None,
                has_auto_increment: false,
                foreign_key: None,
                meta: None,
            }],
            tables: vec![],
            app_versions: vec![],
        }
    }

    fn render(expr: SimpleExpr) -> String {
        match expr {
            SimpleExpr::Custom(sql) => sql,
            _ => panic!("expected a custom expression"),
        }
    }

    #[test]
    fn uuid_array_parses_to_validated_literal() {
        let schema = schema_with_uuid_array();
        let id = "0fe48201-e63e-44fa-a272-ac007572a8c1";
        let expr = parse_value(
            &schema,
            "shop010production",
            &vec![],
            "products",
            "photo",
            json!([id]),
        )
        .expect("array should parse");
        let sql = render(expr);
        assert!(sql.contains("ARRAY["), "{sql}");
        assert!(sql.contains("::uuid"), "{sql}");
        assert!(sql.contains(id), "{sql}");
    }

    #[test]
    fn empty_uuid_array_is_valid() {
        let schema = schema_with_uuid_array();
        let expr = parse_value(
            &schema,
            "shop010production",
            &vec![],
            "products",
            "photo",
            json!([]),
        )
        .expect("empty array should parse");
        assert_eq!(render(expr), "ARRAY[]::uuid[]");
    }

    #[test]
    fn non_uuid_elements_are_rejected() {
        let schema = schema_with_uuid_array();
        let expr = parse_value(
            &schema,
            "shop010production",
            &vec![],
            "products",
            "photo",
            json!(["not-a-uuid"]),
        );
        assert!(expr.is_none());
    }
}

