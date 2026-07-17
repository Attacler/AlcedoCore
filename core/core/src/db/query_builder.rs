use serde::{Deserialize, Serialize};

use crate::db::schema::{ColumnSchema, TableSchema};
use crate::db::Pool;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    StartsWith,
    EndsWith,
    #[serde(rename = "in")]
    In,
    #[serde(rename = "not_in")]
    NotIn,
    #[serde(rename = "null")]
    Null,
    #[serde(rename = "not_null")]
    NotNull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterCondition {
    pub field: Option<String>,
    pub operator: Option<FilterOperator>,
    pub value: Option<serde_json::Value>,
    pub combinator: Option<String>,
    pub conditions: Option<Vec<FilterCondition>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortField {
    pub field: String,
    #[serde(default = "default_sort_direction")]
    pub direction: String,
}

fn default_sort_direction() -> String {
    "asc".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub select: Option<Vec<String>>,
    pub filters: Option<FilterCondition>,
    pub sort: Option<Vec<SortField>>,
    #[serde(default = "default_limit")]
    pub limit: u64,
    #[serde(default = "default_offset")]
    pub offset: u64,
    #[serde(skip)]
    pub extra_select: Option<Vec<String>>,
    #[serde(skip)]
    pub extra_select_binds: Option<Vec<serde_json::Value>>,
}

fn default_limit() -> u64 {
    100
}

fn default_offset() -> u64 {
    0
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<serde_json::Value>,
    pub row_count: usize,
    pub truncated: bool,
}

#[derive(Clone)]
pub(crate) enum BindValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

fn json_to_bind(val: &serde_json::Value) -> BindValue {
    match val {
        serde_json::Value::Null => BindValue::Null,
        serde_json::Value::Bool(b) => BindValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                BindValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                BindValue::Float(f)
            } else {
                BindValue::Int(n.as_i64().unwrap_or(0))
            }
        }
        serde_json::Value::String(s) => BindValue::String(s.clone()),
        serde_json::Value::Array(arr) => BindValue::String(
            arr.iter()
                .map(|v| json_to_bind_value_str(v))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        serde_json::Value::Object(_) => BindValue::String(val.to_string()),
    }
}

fn json_to_bind_value_str(val: &serde_json::Value) -> String {
    match json_to_bind(val) {
        BindValue::String(s) => format!("'{}'", s.replace('\'', "''")),
        BindValue::Int(i) => i.to_string(),
        BindValue::Float(f) => f.to_string(),
        BindValue::Bool(b) => b.to_string(),
        BindValue::Null => "NULL".to_string(),
    }
}

fn make_placeholder(idx: usize, val: &BindValue) -> String {
    match val {
        BindValue::Null => "NULL".to_string(),
        BindValue::String(_) => format!("${}::text", idx),
        BindValue::Int(_) => format!("${}::bigint", idx),
        BindValue::Float(_) => format!("${}::double precision", idx),
        BindValue::Bool(_) => format!("${}::boolean", idx),
    }
}

pub fn validate_query_request(
    request: &QueryRequest,
    table_schema: &TableSchema,
) -> Result<(), AppError> {
    if let Some(ref select_fields) = request.select {
        for field in select_fields {
            let exists = table_schema.columns.iter().any(|c| c.column_name == *field);
            if !exists {
                return Err(AppError::BadRequest(format!(
                    "Invalid column in select: '{}'. Not found in table '{}'",
                    field, table_schema.table_name
                )));
            }
        }
    }
    if let Some(ref sort_fields) = request.sort {
        for sort in sort_fields {
            if sort.direction != "asc" && sort.direction != "desc" {
                return Err(AppError::BadRequest(format!(
                    "Invalid sort direction: '{}'. Must be 'asc' or 'desc'",
                    sort.direction
                )));
            }
            let exists = table_schema.columns.iter().any(|c| c.column_name == sort.field);
            if !exists {
                return Err(AppError::BadRequest(format!(
                    "Invalid sort column: '{}'. Not found in table '{}'",
                    sort.field, table_schema.table_name
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn build_select_query(
    table_schema: &TableSchema,
    request: &QueryRequest,
    schema: &str,
    table_name: &str,
) -> Result<(String, Vec<BindValue>), AppError> {
    validate_query_request(request, table_schema)?;

    let columns = &table_schema.columns;
    let mut binds: Vec<BindValue> = Vec::new();
    let mut sql = String::new();

    sql.push_str("SELECT ");
    if let Some(ref extra_select) = request.extra_select {
        sql.push_str(&extra_select.join(", "));
    } else if let Some(ref select_fields) = request.select {
        if select_fields.is_empty() {
            let cols: Vec<String> = columns.iter().map(|c| format!("\"{}\"", c.column_name)).collect();
            sql.push_str(&cols.join(", "));
        } else {
            let cols: Vec<String> = select_fields
                .iter()
                .map(|f| format!("\"{}\"", f))
                .collect();
            sql.push_str(&cols.join(", "));
        }
    } else {
        let cols: Vec<String> = columns.iter().map(|c| format!("\"{}\"", c.column_name)).collect();
        sql.push_str(&cols.join(", "));
    }

    sql.push_str(&format!(" FROM \"{}\".\"{}\"", schema, table_name));

    // Bind values for extra_select CASE WHEN expressions (come before WHERE binds)
    if let Some(ref extra_binds) = request.extra_select_binds {
        for val in extra_binds {
            binds.push(json_to_bind(val));
        }
    }

    if let Some(ref filters) = request.filters {
        let (clause, mut filter_binds) = build_where(filters, columns, 1)?;
        if !clause.is_empty() {
            sql.push_str(&format!(" WHERE {}", clause));
            binds.append(&mut filter_binds);
        }
    }

    if let Some(ref sort_fields) = request.sort {
        if !sort_fields.is_empty() {
            let sorts: Vec<String> = sort_fields
                .iter()
                .map(|s| format!("\"{}\" {}", s.field, s.direction.to_uppercase()))
                .collect();
            sql.push_str(&format!(" ORDER BY {}", sorts.join(", ")));
        }
    }

    sql.push_str(&format!(" LIMIT {}", request.limit));
    sql.push_str(&format!(" OFFSET {}", request.offset));

    Ok((sql, binds))
}

fn build_where(
    filter: &FilterCondition,
    columns: &[ColumnSchema],
    next_idx: usize,
) -> Result<(String, Vec<BindValue>), AppError> {
    if let Some(ref combinator) = filter.combinator {
        let conditions = filter.conditions.as_ref().ok_or_else(|| {
            AppError::BadRequest("Group filter must have 'conditions' array".to_string())
        })?;

        let mut idx = next_idx;
        let mut binds = Vec::new();
        let mut parts = Vec::new();

        for child in conditions {
            let (clause, mut child_binds) = build_where(child, columns, idx)?;
            idx += child_binds.len();
            binds.append(&mut child_binds);
            parts.push(clause);
        }

        if parts.is_empty() {
            return Ok((String::new(), binds));
        }

        let sep = match combinator.to_lowercase().as_str() {
            "and" => " AND ",
            "or" => " OR ",
            other => {
                return Err(AppError::BadRequest(format!(
                    "Invalid combinator: '{}'. Use 'and' or 'or'",
                    other
                )));
            }
        };

        let clause = if parts.len() == 1 {
            parts.into_iter().next().unwrap()
        } else {
            format!("({})", parts.join(sep))
        };

        Ok((clause, binds))
    } else {
        let field = filter.field.as_ref().ok_or_else(|| {
            AppError::BadRequest("Leaf filter missing 'field'".to_string())
        })?;
        let operator = filter.operator.as_ref().ok_or_else(|| {
            AppError::BadRequest("Leaf filter missing 'operator'".to_string())
        })?;

        if !columns.iter().any(|c| c.column_name == *field) {
            return Err(AppError::BadRequest(format!(
                "Invalid filter column: '{}'",
                field
            )));
        }

        match operator {
            FilterOperator::Null => Ok((format!("\"{}\" IS NULL", field), vec![])),
            FilterOperator::NotNull => Ok((format!("\"{}\" IS NOT NULL", field), vec![])),
            _ => {
                let val = filter.value.as_ref().ok_or_else(|| {
                    AppError::BadRequest("Filter missing 'value'".to_string())
                })?;

                match operator {
                    FilterOperator::In | FilterOperator::NotIn => {
                        let arr = val.as_array().ok_or_else(|| {
                            AppError::BadRequest("'in'/'not_in' operators require an array value".to_string())
                        })?;

                        let binds: Vec<BindValue> = arr.iter().map(json_to_bind).collect();
                        let phs: Vec<String> = binds
                            .iter()
                            .enumerate()
                            .map(|(i, b)| make_placeholder(next_idx + i, b))
                            .collect();

                        let clause = if matches!(operator, FilterOperator::In) {
                            format!("\"{}\" IN ({})", field, phs.join(", "))
                        } else {
                            format!("\"{}\" NOT IN ({})", field, phs.join(", "))
                        };

                        Ok((clause, binds))
                    }
                    FilterOperator::Contains | FilterOperator::StartsWith | FilterOperator::EndsWith => {
                        let raw = json_to_string(val);
                        let pattern = match operator {
                            FilterOperator::Contains => format!("%{}%", raw),
                            FilterOperator::StartsWith => format!("{}%", raw),
                            FilterOperator::EndsWith => format!("%{}", raw),
                            _ => unreachable!(),
                        };
                        let clause = format!(
                            "\"{}\" LIKE ${}::text",
                            field,
                            next_idx
                        );
                        Ok((clause, vec![BindValue::String(pattern)]))
                    }
                    _ => {
                        let bv = json_to_bind(val);
                        let ph = make_placeholder(next_idx, &bv);
                        let op_str = match operator {
                            FilterOperator::Eq => "=",
                            FilterOperator::Neq => "!=",
                            FilterOperator::Gt => ">",
                            FilterOperator::Gte => ">=",
                            FilterOperator::Lt => "<",
                            FilterOperator::Lte => "<=",
                            _ => unreachable!(),
                        };
                        let clause = format!("\"{}\" {} {}", field, op_str, ph);
                        Ok((clause, vec![bv]))
                    }
                }
            }
        }
    }
}

fn json_to_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub async fn execute_query(
    pool: &Pool,
    schema: &str,
    table_schema: &TableSchema,
    request: &QueryRequest,
) -> Result<QueryResponse, AppError> {
    let (sql, binds) = build_select_query(table_schema, request, schema, &table_schema.table_name)?;

    let max_rows: u64 = 100000;
    let wrapped_sql = format!(
        "SELECT COALESCE(json_agg(\"_query\"), '[]'::json) FROM ({}) AS \"_query\" LIMIT {}",
        sql.trim_end_matches(';'),
        max_rows
    );

    let mut db_query = sqlx::query_as::<_, (serde_json::Value,)>(&wrapped_sql);
    for bind in &binds {
        match bind {
            BindValue::String(s) => {
                db_query = db_query.bind(s.clone());
            }
            BindValue::Int(i) => {
                db_query = db_query.bind(*i);
            }
            BindValue::Float(f) => {
                db_query = db_query.bind(*f);
            }
            BindValue::Bool(b) => {
                db_query = db_query.bind(*b);
            }
            BindValue::Null => {
                let val: Option<String> = None;
                db_query = db_query.bind(val);
            }
        }
    }

    let result: (serde_json::Value,) = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        db_query.fetch_one(pool),
    )
    .await
    .map_err(|_| AppError::Internal("Query timeout after 60 seconds".to_string()))?
    .map_err(|e| AppError::DatabaseError {
        details: format!("Query execution failed: {}", e),
    })?;

    let rows: Vec<serde_json::Value> = match result.0 {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    let row_count = rows.len();
    let truncated = row_count as u64 >= max_rows;

    let columns = request
        .select
        .clone()
        .unwrap_or_else(|| table_schema.columns.iter().map(|c| c.column_name.clone()).collect());

    Ok(QueryResponse {
        columns,
        rows,
        row_count,
        truncated,
    })
}
