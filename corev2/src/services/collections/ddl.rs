use sea_query::{
    Alias, ColumnDef, Expr, ForeignKey, ForeignKeyAction, PostgresQueryBuilder, SimpleExpr, Table,
};
use serde_json::Value;

use crate::services::{context::AppContext, errors::AlcedoError};

use super::FieldDefinition;

pub(crate) enum ColumnSql {
    Varchar(Option<u32>),
    Text,
    Integer,
    Double,
    Decimal(u32, u32),
    Boolean,
    Date,
    Timestamp,
    Timestamptz,
    Uuid,
    UuidArray,
    Jsonb,
}

pub(crate) struct ColumnSpec {
    pub name: String,
    pub sql: ColumnSql,
    pub not_null: bool,
    pub auto_increment: bool,
    pub primary_key: bool,
    pub default: Option<SimpleExpr>,
}

pub(crate) fn column_def(spec: ColumnSpec) -> ColumnDef {
    let mut col = ColumnDef::new(Alias::new(&spec.name));
    match spec.sql {
        ColumnSql::Varchar(len) => match len {
            Some(n) => {
                col.string_len(n);
            }
            None => {
                col.string();
            }
        },
        ColumnSql::Text => {
            col.text();
        }
        ColumnSql::Integer => {
            col.integer();
        }
        ColumnSql::Double => {
            col.double();
        }
        ColumnSql::Decimal(p, s) => {
            col.decimal_len(p, s);
        }
        ColumnSql::Boolean => {
            col.boolean();
        }
        ColumnSql::Date => {
            col.date();
        }
        ColumnSql::Timestamp => {
            col.timestamp();
        }
        ColumnSql::Timestamptz => {
            col.timestamp_with_time_zone();
        }
        ColumnSql::Uuid => {
            col.uuid();
        }
        ColumnSql::UuidArray => {
            col.custom(Alias::new("UUID[]"));
        }
        ColumnSql::Jsonb => {
            col.json_binary();
        }
    }
    if spec.not_null {
        col.not_null();
    }
    if spec.auto_increment {
        col.auto_increment();
    }
    if spec.primary_key {
        col.primary_key();
    }
    if let Some(expr) = spec.default {
        col.default(expr);
    }
    col
}

pub(crate) fn spec_from_field_creation(
    f: &super::schema::FieldCreationObject,
) -> Result<ColumnSpec, AlcedoError> {
    let simple = |v: &Option<String>| match v {
        Some(s) if !s.is_empty() => Some(Expr::value(s.clone())),
        _ => None,
    };
    let (sql, default) = match f.col_type.as_str() {
        "Integer" => (ColumnSql::Integer, simple(&f.default_value)),
        "Float" => match (f.numeric_precision, f.numeric_scale) {
            (Some(p), Some(s)) => (ColumnSql::Decimal(p, s), simple(&f.default_value)),
            _ => (ColumnSql::Double, simple(&f.default_value)),
        },
        "Boolean" => (ColumnSql::Boolean, simple(&f.default_value)),
        "Date" => (
            ColumnSql::Date,
            match f.default_value.as_deref() {
                Some("CURRENT_DATE") => Some(Expr::cust("CURRENT_DATE")),
                Some(v) => Some(Expr::value(v.to_string())),
                None => None,
            },
        ),
        "DateTime" => (
            ColumnSql::Timestamp,
            match f.default_value.as_deref() {
                Some("CURRENT_TIMESTAMP") => Some(Expr::cust("CURRENT_TIMESTAMP")),
                Some("NOW()") => Some(Expr::cust("NOW()")),
                Some(v) => Some(Expr::value(v.to_string())),
                None => None,
            },
        ),
        "JSONB" => (ColumnSql::Jsonb, simple(&f.default_value)),
        _ => (ColumnSql::Varchar(f.max_length), simple(&f.default_value)),
    };
    Ok(ColumnSpec {
        name: f.name.clone(),
        sql,
        not_null: f.is_nullable != Some(true),
        auto_increment: f.has_auto_increment.unwrap_or(false),
        primary_key: false,
        default,
    })
}

pub(crate) fn spec_from_field(field: &FieldDefinition) -> Result<ColumnSpec, AlcedoError> {
    let sql = match field.field_type.as_str() {
        "string" => ColumnSql::Varchar(Some(255)),
        "text" => ColumnSql::Text,
        "int" => ColumnSql::Integer,
        "float" => ColumnSql::Double,
        "datetime" => ColumnSql::Timestamptz,
        "uuid" => ColumnSql::Uuid,
        "relationship" => ColumnSql::Uuid,
        "boolean" => ColumnSql::Boolean,
        "file" => ColumnSql::UuidArray,
        _ => ColumnSql::Text,
    };
    let default = match &field.default_value {
        Some(dv) if !dv.is_null() => Some(default_expr(field, dv)?),
        _ => None,
    };
    Ok(ColumnSpec {
        name: field.name.clone(),
        sql,
        not_null: field.required,
        auto_increment: false,
        primary_key: false,
        default,
    })
}

pub(crate) fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

pub(crate) fn qtable(ctx: &AppContext, table: &str) -> String {
    format!("{}.{}", quote(&ctx.schema_name()), quote(table))
}

pub(crate) fn related_app_schema(ctx: &AppContext, related_app: &Option<String>) -> String {
    let app = related_app.clone().unwrap_or_else(|| ctx.app_api_name());
    format!(
        "{}010{}",
        crate::utils::slugify(&app),
        ctx.version_api_name()
    )
}

pub(crate) fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

pub(crate) fn default_expr(
    field: &FieldDefinition,
    value: &Value,
) -> Result<SimpleExpr, AlcedoError> {
    let invalid = || {
        AlcedoError::InvalidInput(
            format!(
                "Invalid default value for field '{}' of type '{}'",
                field.name, field.field_type
            ),
            1,
        )
    };
    match (field.field_type.as_str(), value) {
        ("string", Value::String(s)) | ("text", Value::String(s)) => Ok(Expr::value(s.clone())),
        ("int", Value::Number(n)) => n
            .as_i64()
            .map(|i| Expr::value(i as i32))
            .ok_or_else(invalid),
        ("int", Value::String(s)) => s.parse::<i32>().map(Expr::value).map_err(|_| invalid()),
        ("float", Value::Number(n)) => n.as_f64().map(Expr::value).ok_or_else(invalid),
        ("float", Value::String(s)) => s.parse::<f64>().map(Expr::value).map_err(|_| invalid()),
        ("datetime", Value::String(s)) => Ok(Expr::cust(format!(
            "'{}'::timestamptz",
            escape_sql_string(s)
        ))),
        ("uuid", Value::String(s)) => Ok(Expr::cust(format!("'{}'::uuid", escape_sql_string(s)))),
        ("boolean", Value::Bool(b)) => Ok(Expr::value(*b)),
        ("boolean", Value::String(s)) if s == "true" || s == "false" => Ok(Expr::cust(s.clone())),
        ("relationship", _) => Err(AlcedoError::InvalidInput(
            format!(
                "Relationship field '{}' does not support default values",
                field.name
            ),
            1,
        )),
        ("file", _) => Err(AlcedoError::InvalidInput(
            format!(
                "File field '{}' does not support default values",
                field.name
            ),
            1,
        )),
        _ => Err(invalid()),
    }
}

pub(crate) fn field_column_def(field: &FieldDefinition) -> Result<ColumnDef, AlcedoError> {
    Ok(column_def(spec_from_field(field)?))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fk_constraint(
    name: &str,
    from_schema: &str,
    from_table: &str,
    from_col: &str,
    to_schema: &str,
    to_table: &str,
    to_col: &str,
    on_delete: ForeignKeyAction,
    on_update: ForeignKeyAction,
) -> String {
    ForeignKey::create()
        .name(name.to_string())
        .from(
            (Alias::new(from_schema), Alias::new(from_table)),
            Alias::new(from_col),
        )
        .to(
            (Alias::new(to_schema), Alias::new(to_table)),
            Alias::new(to_col),
        )
        .on_delete(on_delete)
        .on_update(on_update)
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

pub(crate) fn drop_table_sql(ctx: &AppContext, table: &str, cascade: bool) -> String {
    let mut stmt = Table::drop()
        .table((Alias::new(ctx.schema_name()), Alias::new(table)))
        .if_exists()
        .to_owned();
    if cascade {
        stmt.cascade();
    }
    stmt.to_string(PostgresQueryBuilder)
}

pub(crate) fn build_create_table_sql(
    ctx: &AppContext,
    table: &str,
    fields: &[FieldDefinition],
) -> Result<String, AlcedoError> {
    let mut stmt = Table::create()
        .table((Alias::new(ctx.schema_name()), Alias::new(table)))
        .to_owned();

    stmt.col(
        ColumnDef::new(Alias::new("id"))
            .uuid()
            .not_null()
            .primary_key()
            .default(Expr::cust("gen_random_uuid()")),
    );
    stmt.col(
        ColumnDef::new(Alias::new("created_at"))
            .timestamp_with_time_zone()
            .not_null()
            .default(Expr::cust("NOW()")),
    );
    stmt.col(
        ColumnDef::new(Alias::new("updated_at"))
            .timestamp_with_time_zone()
            .not_null()
            .default(Expr::cust("NOW()")),
    );
    for field in fields.iter().filter(|f| !f.is_virtual()) {
        stmt.col(field_column_def(field)?);
    }
    Ok(stmt.to_string(PostgresQueryBuilder))
}

pub(crate) fn build_add_columns_sqls(
    ctx: &AppContext,
    table: &str,
    fields: &[&FieldDefinition],
) -> Result<Vec<String>, AlcedoError> {
    let mut out = Vec::new();
    for field in fields.iter().copied().filter(|f| !f.is_virtual()) {
        let sql = Table::alter()
            .table((Alias::new(ctx.schema_name()), Alias::new(table)))
            .add_column(field_column_def(field)?)
            .to_string(PostgresQueryBuilder);
        out.push(sql);
    }
    Ok(out)
}

pub(crate) fn build_drop_columns_sqls(
    ctx: &AppContext,
    table: &str,
    names: &[&str],
) -> Vec<String> {
    names
        .iter()
        .map(|name| {
            Table::alter()
                .table((Alias::new(ctx.schema_name()), Alias::new(table)))
                .drop_column(Alias::new(*name))
                .to_string(PostgresQueryBuilder)
        })
        .collect()
}

pub(crate) fn build_rename_columns_sqls(
    ctx: &AppContext,
    table: &str,
    renames: &[(&str, &str)],
) -> Vec<String> {
    renames
        .iter()
        .map(|(old, new)| {
            Table::alter()
                .table((Alias::new(ctx.schema_name()), Alias::new(table)))
                .rename_column(Alias::new(*old), Alias::new(*new))
                .to_string(PostgresQueryBuilder)
        })
        .collect()
}

pub(crate) fn build_add_fk_sqls(
    ctx: &AppContext,
    table: &str,
    fields: &[&FieldDefinition],
) -> Result<Vec<String>, AlcedoError> {
    let mut out = Vec::new();
    for field in fields.iter().copied() {
        if !field.is_relationship() || field.is_virtual() {
            continue;
        }
        let related = field.related_collection.clone().ok_or_else(|| {
            AlcedoError::InvalidInput(
                format!(
                    "Relationship field '{}' missing related_collection",
                    field.name
                ),
                1,
            )
        })?;
        let target_schema = related_app_schema(ctx, &field.related_app);
        out.push(fk_constraint(
            &format!("fk_{}_{}", table, field.name),
            &ctx.schema_name(),
            table,
            &field.name,
            &target_schema,
            &related,
            "id",
            ForeignKeyAction::Restrict,
            ForeignKeyAction::Restrict,
        ));
    }
    Ok(out)
}

pub(crate) fn build_drop_fk_sqls(ctx: &AppContext, table: &str, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|name| {
            format!(
                "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {}",
                qtable(ctx, table),
                quote(&format!("fk_{}_{}", table, name))
            )
        })
        .collect()
}

pub(crate) fn unique_index_name(table: &str, field: &str) -> String {
    format!("uniq_{}_{}", table, field)
}

pub(crate) fn build_add_unique_sql(ctx: &AppContext, table: &str, field: &str) -> String {
    // The index name is intentionally NOT schema-qualified: `CREATE INDEX`
    // rejects a qualified index name and creates the index in the table's
    // schema anyway.
    format!(
        "CREATE UNIQUE INDEX IF NOT EXISTS {} ON {} ({})",
        quote(&unique_index_name(table, field)),
        qtable(ctx, table),
        quote(field)
    )
}

pub(crate) fn build_drop_unique_sql(ctx: &AppContext, table: &str, field: &str) -> String {
    format!(
        "DROP INDEX IF EXISTS {}.{}",
        quote(&ctx.schema_name()),
        quote(&unique_index_name(table, field))
    )
}

pub(crate) fn build_not_null_sql(
    ctx: &AppContext,
    table: &str,
    field: &str,
    not_null: bool,
) -> String {
    format!(
        "ALTER TABLE {} ALTER COLUMN {} {}",
        qtable(ctx, table),
        quote(field),
        if not_null {
            "SET NOT NULL"
        } else {
            "DROP NOT NULL"
        }
    )
}

pub(crate) fn build_set_default_sql(
    ctx: &AppContext,
    table: &str,
    field: &FieldDefinition,
    value: &Value,
) -> Result<String, AlcedoError> {
    let expr = default_expr(field, value)?;
    let sql = Table::alter()
        .table((Alias::new(ctx.schema_name()), Alias::new(table)))
        .modify_column(ColumnDef::new(Alias::new(&field.name)).default(expr))
        .to_string(PostgresQueryBuilder);
    Ok(sql)
}

pub(crate) fn build_drop_default_sql(ctx: &AppContext, table: &str, field: &str) -> String {
    format!(
        "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT",
        qtable(ctx, table),
        quote(field)
    )
}
