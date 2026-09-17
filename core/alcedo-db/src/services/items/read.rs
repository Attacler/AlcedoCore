use std::collections::HashMap;

use serde_json::Value;

use alcedo_common::error::AppError;

use crate::db::collection_items;
use crate::db::collections::{self, CollectionDefinition, FieldType};
use crate::db::filter_compiler;
use crate::db::filter_condition::{FilterCondition, SortField};
use crate::db::field_resolver::{self, FieldResolverOptions};
use crate::services::items::shape::{column_cast, TableShape};
use crate::services::permissions::{self, PolicyPermission};

/// The flat table read always emits `LIMIT`; the integer maximum stands in for
/// "no limit" (preserves the pre-engine unbounded `SELECT` semantics).
pub const UNBOUNDED_LIMIT: u64 = i64::MAX as u64;

/// List read request. `permissions` empty => unrestricted (admin/bypass).
#[derive(Debug, Clone)]
pub struct ListRequest {
    pub fields: Vec<String>,
    pub filter: Option<FilterCondition>,
    pub sort: Vec<SortField>,
    pub limit: u64,
    pub offset: u64,
    pub backlink: bool,
    pub depth_limit: usize,
    pub permissions: Vec<PolicyPermission>,
    /// Gates BOTH relationship display-value augmentation and file-metadata
    /// augmentation. `/api/items/:slug/query` sets this to `false` (raw rows),
    /// while `/api/items/:slug` sets it to `true`.
    pub augment: bool,
}

impl Default for ListRequest {
    fn default() -> Self {
        Self {
            fields: Vec::new(),
            filter: None,
            sort: Vec::new(),
            limit: 100,
            offset: 0,
            backlink: false,
            depth_limit: 5,
            permissions: Vec::new(),
            augment: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListResult {
    pub items: Vec<Value>,
    pub total: i64,
}

#[derive(Debug, Clone)]
pub struct OneRequest {
    pub item_id: String,
    pub fields: Vec<String>,
    pub backlink: bool,
    pub depth_limit: usize,
}

impl Default for OneRequest {
    fn default() -> Self {
        Self {
            item_id: String::new(),
            fields: Vec::new(),
            backlink: false,
            depth_limit: 5,
        }
    }
}

/// Raw single-item fetch by id; returns `None` when not found. No augmentation.
pub async fn execute_one(
    pool: &crate::db::Pool,
    collection_name: &str,
    request: OneRequest,
) -> Result<Option<Value>, AppError> {
    let collection = collections::get_collection(pool, collection_name).await?;
    let quoted = format!("\"{}\"", collection_name);
    let fields_param = request.fields.clone();

    let sql = if !fields_param.is_empty() {
        let flat_fields: Vec<&String> = fields_param.iter().filter(|f| !f.contains('.')).collect();
        let implicit_cols = ["id", "created_at", "updated_at"];
        let mut all_names: Vec<String> =
            collection.fields.iter().map(|f| f.name.clone()).collect();
        for col in &implicit_cols {
            if !all_names.contains(&col.to_string()) {
                all_names.push(col.to_string());
            }
        }
        for field_name in &flat_fields {
            if !all_names.iter().any(|n| n == *field_name) {
                return Err(AppError::BadRequest(format!("Unknown field: '{}'", field_name)));
            }
        }
        let mut cols: Vec<String> = if flat_fields.is_empty() {
            all_names.iter().map(|n| format!("\"{}\"", n)).collect()
        } else {
            let mut names: Vec<String> = flat_fields.iter().map(|s| (*s).clone()).collect();
            for col in &implicit_cols {
                if !names.contains(&col.to_string()) {
                    names.push(col.to_string());
                }
            }
            names.iter().map(|n| format!("\"{}\"", n)).collect()
        };
        if fields_param.iter().any(|f| f.contains('.')) {
            let all_collections = collections::list_collections(pool).await?;
            let mut field_opts = FieldResolverOptions {
                depth_limit: request.depth_limit,
                backlink: request.backlink,
                visited: std::collections::HashSet::new(),
                ..Default::default()
            };
            let fragments = field_resolver::resolve_nested_fields(
                &fields_param,
                collection_name,
                &all_collections,
                &mut field_opts,
            )?;
            for fragment in &fragments {
                cols.push(fragment.select_clause.clone());
            }
        }
        format!(
            r#"SELECT row_to_json("_q".*) FROM (SELECT {} FROM {} WHERE "id" = $1::uuid) AS "_q""#,
            cols.join(", "),
            quoted,
        )
    } else {
        format!(
            r#"SELECT row_to_json({}.*) FROM {} WHERE "id" = $1::uuid"#,
            quoted, quoted,
        )
    };

    let row: Option<(Value,)> = sqlx::query_as(&sql)
        .bind(&request.item_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Collection item query failed: {}", e),
        })?;

    Ok(row.map(|(value,)| value))
}

/// True when any leaf filter field contains a dot (needs JOINs).
pub(crate) fn filter_has_dot_path(filter: &FilterCondition) -> bool {
    match filter {
        FilterCondition::Group { conditions, .. } => conditions.iter().any(filter_has_dot_path),
        FilterCondition::Rule { field, .. } => field.contains('.'),
    }
}

pub async fn execute_grouped(
    pool: &crate::db::Pool,
    collection: &str,
    request: crate::db::filter_condition::GroupedQueryRequest,
    permissions: &[PolicyPermission],
) -> Result<crate::db::filter_condition::GroupedQueryResponse, AppError> {
    collection_items::grouped_query_items(pool, collection, request, permissions).await
}

/// Group a table resolved as a [`TableShape`]. Collection-backed shapes delegate
/// to [`execute_grouped`]; physical shapes read the qualified table directly.
pub async fn execute_grouped_for_table(
    pool: &crate::db::Pool,
    shape: &TableShape,
    request: crate::db::filter_condition::GroupedQueryRequest,
    permissions: &[PolicyPermission],
) -> Result<crate::db::filter_condition::GroupedQueryResponse, AppError> {
    collection_items::grouped_query_items_for_table(pool, shape, request, permissions).await
}

pub async fn execute_references(
    pool: &crate::db::Pool,
    collection: &str,
    item_id: &str,
    collection_filters: &HashMap<String, Vec<PolicyPermission>>,
) -> Result<Vec<crate::db::collections::ReferencingGroup>, AppError> {
    collections::get_referencing_items(pool, collection, item_id, collection_filters).await
}

/// List items with filter/sort/pagination, applying optional augmentation.
pub async fn execute_list(
    pool: &crate::db::Pool,
    collection: &str,
    request: ListRequest,
) -> Result<ListResult, AppError> {
    let collection_name = collection;
    let collection: CollectionDefinition =
        collections::get_collection(pool, collection_name).await?;
    let col_type_map: HashMap<&str, &FieldType> = collection
        .fields
        .iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();

    let has_dot_notation = request.filter.as_ref().map_or(false, filter_has_dot_path);
    let all_collections = collections::list_collections(pool).await?;

    let mut bind_values: Vec<Value> = Vec::new();
    let mut joins: Vec<String> = Vec::new();

    let mut where_clause = match &request.filter {
        Some(filter) => {
            let clause = if has_dot_notation {
                filter_compiler::compile_filter_with_joins(
                    filter,
                    &col_type_map,
                    &mut bind_values,
                    collection_name,
                    &all_collections,
                    &mut joins,
                    None,
                    None,
                )?
            } else {
                filter_compiler::compile_filter(filter, &col_type_map, &mut bind_values)?
            };
            if clause.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", clause)
            }
        }
        None => String::new(),
    };

    // --- Permission row filter (same builders the handlers use today) ---
    if !request.permissions.is_empty() {
        let table_prefix = if joins.is_empty() {
            None
        } else {
            Some(collection_name)
        };
        let (perm_where, perm_binds, perm_joins) = permissions::build_filter_clause_with_joins(
            &request.permissions,
            bind_values.len(),
            table_prefix,
            &collection_name,
            &collection,
            &all_collections,
        );
        if !perm_where.is_empty() {
            joins.extend(perm_joins);
            if where_clause.is_empty() {
                where_clause = format!(" WHERE {}", perm_where);
            } else {
                where_clause = format!("{} AND ({})", where_clause, perm_where);
            }
            bind_values.extend(perm_binds);
        }
    }

    let join_clause = if joins.is_empty() {
        String::new()
    } else {
        format!(" {}", joins.join(" "))
    };

    let order_clause = filter_compiler::build_order_by(&request.sort);

    // --- SELECT expression (field mask + nested fields) ---
    let implicit_cols = ["id", "created_at", "updated_at"];
    let mut all_field_names: Vec<String> = collection
        .fields
        .iter()
        .filter(|f| f.relationship_type.as_deref() != Some("one_to_many"))
        .map(|f| f.name.clone())
        .collect();
    for col in &implicit_cols {
        if !all_field_names.contains(&col.to_string()) {
            all_field_names.push(col.to_string());
        }
    }

    let fields_param = request.fields.clone();
    let fields_has_dot_notation = fields_param.iter().any(|f| f.contains('.'));

    let select_expr = if !fields_param.is_empty() {
        let flat_fields: Vec<&String> = fields_param.iter().filter(|f| !f.contains('.')).collect();
        for field_name in &flat_fields {
            if !all_field_names.iter().any(|n| n == *field_name) {
                return Err(AppError::BadRequest(format!("Unknown field: '{}'", field_name)));
            }
        }

        let mut base_cols: Vec<String>;
        let apply_mask = !request.permissions.is_empty();
        let names_for_expr: Vec<String> = if flat_fields.is_empty() {
            all_field_names.clone()
        } else {
            let mut names: Vec<String> = flat_fields.iter().map(|s| (*s).clone()).collect();
            for col in &implicit_cols {
                if !names.contains(&col.to_string()) {
                    names.push(col.to_string());
                }
            }
            names
        };

        if apply_mask {
            let table_prefix = if joins.is_empty() {
                None
            } else {
                Some(collection_name)
            };
            let (field_exprs, _, mut extra_binds) = permissions::build_field_expressions(
                &request.permissions,
                &names_for_expr,
                table_prefix,
            );
            base_cols = field_exprs;
            if !extra_binds.is_empty() {
                extra_binds.extend(bind_values);
                bind_values = extra_binds;
            }
        } else {
            base_cols = names_for_expr
                .iter()
                .map(|n| {
                    if joins.is_empty() {
                        format!("\"{}\"", n)
                    } else {
                        format!("\"{}\".\"{}\"", collection_name, n)
                    }
                })
                .collect();
        }

        if fields_has_dot_notation {
            let mut field_opts = FieldResolverOptions {
                depth_limit: request.depth_limit,
                backlink: request.backlink,
                visited: std::collections::HashSet::new(),
                ..Default::default()
            };
            let fragments = field_resolver::resolve_nested_fields(
                &fields_param,
                &collection_name,
                &all_collections,
                &mut field_opts,
            )?;
            for fragment in &fragments {
                base_cols.push(fragment.select_clause.clone());
            }
        }

        base_cols.join(", ")
    } else if !request.permissions.is_empty() {
        let table_prefix = if joins.is_empty() {
            None
        } else {
            Some(collection_name)
        };
        let (field_exprs, _, mut extra_binds) = permissions::build_field_expressions(
            &request.permissions,
            &all_field_names,
            table_prefix,
        );
        if !extra_binds.is_empty() {
            extra_binds.extend(bind_values);
            bind_values = extra_binds;
        }
        field_exprs.join(", ")
    } else if joins.is_empty() {
        "*".to_string()
    } else {
        format!("\"{}\".*", collection_name)
    };

    let limit = request.limit;
    let offset = request.offset;
    let data_sql = format!(
        "SELECT COALESCE(json_agg(\"_q\"), '[]'::json) FROM (SELECT {} FROM \"{}\"{}{}{} LIMIT {} OFFSET {}) AS \"_q\"",
        select_expr, collection_name, join_clause, where_clause, order_clause, limit, offset
    );
    let count_sql = format!(
        "SELECT COUNT(*) FROM \"{}\"{}{}",
        collection_name, join_clause, where_clause
    );

    let mut data_q = sqlx::query_as::<_, (Value,)>(&data_sql);
    for val in &bind_values {
        data_q = crate::bind_json_value!(data_q, val);
    }
    let (items_result,): (Value,) = data_q.fetch_one(pool).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Collection items query failed: {}", e),
        }
    })?;
    let mut items = match items_result {
        Value::Array(arr) => arr,
        _ => vec![],
    };

    // Display values + file metadata (permission-gated), same as listing.rs.
    if request.augment && !items.is_empty() {
        let display_fields: Vec<&crate::db::collections::FieldDefinition> = collection
            .fields
            .iter()
            .filter(|f| f.field_type == FieldType::Relationship && f.display_field.is_some())
            .collect();
        if !display_fields.is_empty() {
            // Admin/bypass (empty permissions) must pass `None`, matching
            // listing.rs where permissions_map is only built for `Granted`.
            let permissions_map = if request.permissions.is_empty() {
                None
            } else {
                let mut map = HashMap::new();
                map.insert(collection_name.to_string(), request.permissions.clone());
                Some(map)
            };
            items = collection_items::augment_items_with_display_values(
                pool,
                &items,
                &display_fields,
                &collection_name,
                permissions_map.as_ref(),
            )
            .await
            .unwrap_or(items);
        }

        let file_fields: Vec<&crate::db::collections::FieldDefinition> = collection
            .fields
            .iter()
            .filter(|f| f.field_type == FieldType::File)
            .collect();
        if !file_fields.is_empty() {
            items = collection_items::augment_items_with_file_metadata(pool, &items, &file_fields)
                .await
                .unwrap_or(items);
        }
    }

    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values {
        count_q = crate::bind_json_value!(count_q, val);
    }
    let total = count_q.fetch_one(pool).await.map_err(|e| AppError::DatabaseError {
        details: format!("Collection count query failed: {}", e),
    })?;

    Ok(ListResult { items, total })
}

/// List a table resolved as a [`TableShape`]. Collection-backed shapes delegate
/// to [`execute_list`] so metadata behavior is identical; physical shapes read
/// the table directly.
///
/// When `request.fields` is non-empty the projection unions the shape's primary
/// key column(s) and any existing `created_at`/`updated_at` columns, mirroring
/// the collection path, so callers still receive the row identity. When
/// `request.fields` is empty and the shape carries a `readable` allowlist, that
/// allowlist (plus the primary key) is projected instead of `*`.
pub async fn execute_list_for_table(
    pool: &crate::db::Pool,
    shape: &TableShape,
    request: ListRequest,
) -> Result<ListResult, AppError> {
    if let Some(collection) = &shape.collection {
        return execute_list(pool, &collection.name, request).await;
    }

    if !request.permissions.is_empty() {
        return Err(AppError::BadRequest(
            "Permissions are not supported for physical table reads".to_string(),
        ));
    }

    validate_flat_read_fields(&request.fields, shape)?;

    let table = crate::services::items::shape::qualified_table(&shape.schema, &shape.name);
    let col_type_map = shape.col_type_map_ref();

    let mut bind_values: Vec<Value> = Vec::new();
    let where_clause = match &request.filter {
        Some(filter) => {
            let clause = filter_compiler::compile_filter(filter, &col_type_map, &mut bind_values)?;
            if clause.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", clause)
            }
        }
        None => String::new(),
    };

    let order_clause = filter_compiler::build_order_by(&request.sort);
    let select_expr = flat_select_expr(&request.fields, shape);

    let limit = request.limit;
    let offset = request.offset;
    let data_sql = format!(
        "SELECT COALESCE(json_agg(\"_q\"), '[]'::json) FROM (SELECT {} FROM {}{}{} LIMIT {} OFFSET {}) AS \"_q\"",
        select_expr, table, where_clause, order_clause, limit, offset
    );
    let count_sql = format!("SELECT COUNT(*) FROM {}{}", table, where_clause);

    let mut data_q = sqlx::query_as::<_, (Value,)>(&data_sql);
    for val in &bind_values {
        data_q = crate::bind_json_value!(data_q, val);
    }
    let (items_result,): (Value,) = data_q.fetch_one(pool).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Table items query failed: {}", e),
        }
    })?;
    let items = match items_result {
        Value::Array(arr) => arr,
        _ => vec![],
    };

    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values {
        count_q = crate::bind_json_value!(count_q, val);
    }
    let total = count_q.fetch_one(pool).await.map_err(|e| AppError::DatabaseError {
        details: format!("Table count query failed: {}", e),
    })?;

    Ok(ListResult { items, total })
}

/// Fetch a single table row by primary key. Collection-backed shapes delegate
/// to [`execute_one`]; physical shapes read the table directly.
///
/// When `request.fields` is non-empty the projection unions the shape's primary
/// key column(s) and any existing `created_at`/`updated_at` columns, mirroring
/// the collection path. When `request.fields` is empty and the shape carries a
/// `readable` allowlist, that allowlist (plus the primary key) is projected
/// instead of `*`.
pub async fn execute_one_for_table(
    pool: &crate::db::Pool,
    shape: &TableShape,
    request: OneRequest,
) -> Result<Option<Value>, AppError> {
    if let Some(collection) = &shape.collection {
        return execute_one(pool, &collection.name, request).await;
    }

    validate_flat_read_fields(&request.fields, shape)?;

    let table = crate::services::items::shape::qualified_table(&shape.schema, &shape.name);
    let select_expr = flat_select_expr(&request.fields, shape);

    let pk = shape.single_pk()?;

    let sql = format!(
        r#"SELECT row_to_json("_q".*) FROM (SELECT {} FROM {} WHERE {} = $1{}) AS "_q""#,
        select_expr,
        table,
        filter_compiler::quote(&pk.name),
        column_cast(pk),
    );

    let row: Option<(Value,)> = sqlx::query_as(&sql)
        .bind(&request.item_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Table item query failed: {}", e),
        })?;

    Ok(row.map(|(value,)| value))
}

fn flat_select_expr(fields: &[String], shape: &TableShape) -> String {
    if fields.is_empty() {
        return match &shape.readable {
            Some(readable) => {
                let mut names = readable.clone();
                for pk in &shape.pk {
                    if !names.contains(pk) {
                        names.push(pk.clone());
                    }
                }
                names
                    .iter()
                    .map(|field| filter_compiler::quote(field))
                    .collect::<Vec<String>>()
                    .join(", ")
            }
            None => "*".to_string(),
        };
    }

    let mut names = fields.to_vec();
    for pk in &shape.pk {
        if !names.contains(pk) {
            names.push(pk.clone());
        }
    }
    for implicit in ["created_at", "updated_at"] {
        if shape.columns.iter().any(|c| c.name == implicit) && !names.iter().any(|n| n == implicit) {
            names.push(implicit.to_string());
        }
    }

    names
        .iter()
        .map(|field| filter_compiler::quote(field))
        .collect::<Vec<String>>()
        .join(", ")
}

fn validate_flat_read_fields(fields: &[String], shape: &TableShape) -> Result<(), AppError> {
    for field in fields {
        if field.contains('.') {
            return Err(AppError::BadRequest(format!(
                "Nested field '{}' is not supported for table reads",
                field
            )));
        }
        if !shape.columns.iter().any(|c| c.name == *field) {
            return Err(AppError::BadRequest(format!("Unknown field: '{}'", field)));
        }
    }

    Ok(())
}
