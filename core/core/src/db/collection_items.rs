//! Collection items CRUD service layer
//!
//! Provides dynamic CRUD operations on collection items using sea-query for
//! SQL generation. Schema is resolved at request time from `collection_definitions`
//! metadata stored in the database. Field validation rejects unknown field names
//! and reserved system columns (`id`, `created_at`, `updated_at`) for write operations.

use serde::{Deserialize, Serialize};
use sea_query::{Expr, Order, PostgresQueryBuilder, Query, SimpleExpr};

use crate::db::collections::{get_collection, CollectionDefinition, FieldDefinition, FieldType};
use crate::db::filter_compiler::compile_filter;
use crate::db::filter_condition::{GroupResult, GroupedQueryRequest, GroupedQueryResponse};
use crate::db::relational_crud::{self, CrudDirection};
use crate::db::Pool;
use crate::error::AppError;
use crate::services::permissions::PolicyPermission;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// GET query parameters for collection items listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionItemsQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub filter: Option<serde_json::Value>,
    #[serde(default)]
    pub sort_field: Option<String>,
    #[serde(default)]
    pub sort_order: Option<String>,
}

/// POST body — accepts a single item object or an array (untagged deserialization).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CreateItemsBody {
    Single(serde_json::Map<String, serde_json::Value>),
    Multiple(Vec<serde_json::Map<String, serde_json::Value>>),
}

/// PUT body — filter identifies which items to update, `update` contains the
/// new field values.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateItemsBody {
    pub filter: serde_json::Value,
    pub update: serde_json::Map<String, serde_json::Value>,
}

/// DELETE body — either `filter` (equality conditions) or `pk_values` (list of
/// primary-key values). Providing both is an error.
#[derive(Debug, Clone, Deserialize)]
pub struct DeleteItemsBody {
    pub filter: Option<serde_json::Value>,
    pub pk_values: Option<Vec<serde_json::Value>>,
}

// ---------------------------------------------------------------------------
// Schema resolution
// ---------------------------------------------------------------------------

/// Resolve field definitions from `collection_definitions` metadata at request
/// time. If the collection does not exist the `NotFound` error propagates (404).
async fn resolve_collection_fields(
    pool: &Pool,
    name: &str,
) -> Result<CollectionDefinition, AppError> {
    get_collection(pool, name).await
}

// ---------------------------------------------------------------------------
// Field validation (CRUD-06)
// ---------------------------------------------------------------------------

/// System columns that are always present in every collection table.
const RESERVED_COLUMNS: &[&str] = &["id", "created_at", "updated_at"];

/// Validate field names against the collection's field definitions and reject
/// reserved system columns (`id`, `created_at`, `updated_at`).
///
/// This is called for **write** operations (create, update) so that malicious
/// or accidental attempts to set system columns are rejected before reaching
/// the database.
pub(crate) fn validate_fields_for_write(
    keys: &[String],
    collection: &CollectionDefinition,
) -> Result<(), AppError> {
    let valid_names: Vec<&str> = collection
        .fields
        .iter()
        .map(|f| f.name.as_str())
        .collect();

    for key in keys {
        if RESERVED_COLUMNS.contains(&key.as_str()) {
            return Err(AppError::BadRequest(format!(
                "Field '{}' is a reserved system column and cannot be written directly",
                key
            )));
        }
        if !valid_names.contains(&key.as_str()) {
            return Err(AppError::BadRequest(format!(
                "Unknown field: '{}'. Valid fields: [{}]",
                key,
                valid_names.join(", ")
            )));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Type coercion helper
// ---------------------------------------------------------------------------

/// Best-effort type coercion for incoming JSON values based on the declared
/// `FieldType` from the collection definition.
///
/// This is **best-effort** — PostgreSQL will still validate the final types
/// when the INSERT / UPDATE executes.
pub(crate) fn coerce_value(value: serde_json::Value, field_type: &FieldType) -> serde_json::Value {
    match field_type {
        FieldType::Int => match value {
            serde_json::Value::Number(_) => value,
            serde_json::Value::String(s) => {
                if let Ok(i) = s.parse::<i64>() {
                    serde_json::Value::Number(i.into())
                } else {
                    serde_json::Value::String(s)
                }
            }
            _ => value,
        },
        FieldType::Float => match value {
            serde_json::Value::Number(_) => value,
            serde_json::Value::String(s) => {
                if let Ok(f) = s.parse::<f64>() {
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::String(s))
                } else {
                    serde_json::Value::String(s)
                }
            }
            _ => value,
        },
        FieldType::Datetime | FieldType::Uuid => {
            // Pass through as-is; PostgreSQL will validate the cast.
            value
        }
        FieldType::String | FieldType::Text => match value {
            serde_json::Value::Number(n) => serde_json::Value::String(n.to_string()),
            serde_json::Value::Bool(b) => serde_json::Value::String(b.to_string()),
            _ => value,
        },
        // Relationship fields store UUID values — pass through as-is
        FieldType::Relationship => value,
        // File fields: convert UUID array (or single UUID string) to PostgreSQL array literal
        FieldType::File => match value {
            serde_json::Value::String(s) => {
                if s.is_empty() {
                    serde_json::Value::String("{}".to_string())
                } else {
                    serde_json::Value::String(format!("{{{}}}", s))
                }
            }
            serde_json::Value::Array(arr) => {
                let uuids: Vec<String> = arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                if uuids.is_empty() {
                    serde_json::Value::String("{}".to_string())
                } else {
                    serde_json::Value::String(format!("{{{}}}", uuids.join(",")))
                }
            }
            serde_json::Value::Null => value,
            _ => value,
        },
        FieldType::Bool => match value {
            serde_json::Value::Bool(_) => value,
            serde_json::Value::String(s) => {
                serde_json::Value::Bool(s == "true" || s == "1")
            }
            serde_json::Value::Number(n) => {
                serde_json::Value::Bool(n.as_i64().map_or(false, |i| i != 0))
            }
            _ => value,
        },
    }
}

/// Extract file UUID strings from a File field value. Accepts either a single
/// UUID string or an array of UUID strings (the two shapes the frontend emits).
pub(crate) fn file_ids_from_value(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) if !s.is_empty() => vec![s.clone()],
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// CRUD operations
// ---------------------------------------------------------------------------

/// Query items with pagination and optional simple-equality filter.
///
/// Returns a `Vec<serde_json::Value>` — one value per matching row.
/// Uses sea-query `Query::select()` for the SQL skeleton and wraps the
/// result with `COALESCE(json_agg(…), '[]'::json)`.
///
/// Filter keys (when provided) are accepted for any column — both user-defined
/// fields and implicit system columns (`id`, `created_at`, `updated_at`) are
/// valid filter targets on reads (T-24-03 disposition: accept).
pub async fn query_items(
    pool: &Pool,
    collection_name: &str,
    query: CollectionItemsQuery,
) -> Result<Vec<serde_json::Value>, AppError> {
    // Resolve schema (validates collection exists — 404 if missing).
    let collection = resolve_collection_fields(pool, collection_name).await?;

    // Build col_name -> field_type map for UUID cast detection in filter bindings.
    // Include implicit columns (id, created_at, updated_at) with their fixed types.
    let mut col_type_map: std::collections::HashMap<String, FieldType> = std::collections::HashMap::new();
    for f in &collection.fields {
        col_type_map.insert(f.name.clone(), f.field_type.clone());
    }
    col_type_map.insert("id".to_string(), FieldType::Uuid);
    col_type_map.insert("created_at".to_string(), FieldType::Datetime);
    col_type_map.insert("updated_at".to_string(), FieldType::Datetime);

    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    // Find relationship fields with display_field configured for display value fetching
    let display_fields: Vec<&FieldDefinition> = collection.fields.iter()
        .filter(|f| f.field_type == FieldType::Relationship && f.display_field.is_some())
        .collect();

    // -- Build SELECT with sea-query ---------------------------------------
    // Exclude hidden fields from query results
    let visible_fields: Vec<&FieldDefinition> = collection.fields.iter().filter(|f| !f.hidden).collect();
    let mut select = Query::select();
    for field in &visible_fields {
        select.expr(Expr::col(sea_query::Alias::new(&field.name)));
    }
    for col in &["id", "created_at", "updated_at"] {
        select.expr(Expr::col(sea_query::Alias::new(*col)));
    }
    select.from(sea_query::Alias::new(collection_name));

    let mut bind_values: Vec<serde_json::Value> = Vec::new();

    // Apply filter as simple equality or contains operator WHERE conditions.
    // UUID-typed columns get $N::uuid cast so string values work.
    if let Some(ref filter_json) = query.filter {
        if let Some(obj) = filter_json.as_object() {
            for (key, val) in obj.iter() {
                let idx = bind_values.len() as u32 + 1;
                let quoted_key = super::quote_identifier(key);
                let val_str = val.as_str().unwrap_or("");
                if let Some(search_term) = val_str.strip_prefix("contains:") {
                    let condition = SimpleExpr::Custom(format!("{} LIKE ${}", quoted_key, idx));
                    select.and_where(condition);
                    bind_values.push(serde_json::Value::String(format!("%{}%", search_term)));
                } else {
                    let placeholder = match col_type_map.get(key.as_str()) {
                        Some(FieldType::Uuid) | Some(FieldType::Relationship) => format!("${}::uuid", idx),
                        Some(FieldType::Datetime) => format!("${}::timestamptz", idx),
                        _ => format!("${}", idx),
                    };
                    let condition = SimpleExpr::Custom(format!("{} = {}", quoted_key, placeholder));
                    select.and_where(condition);
                    bind_values.push(val.clone());
                }
            }
        }
    }

    if let Some(ref sf) = query.sort_field {
        let quoted = sea_query::Alias::new(super::quote_identifier(sf).trim_matches('"'));
        let order = match query.sort_order.as_deref() {
            Some("desc") => Order::Desc,
            _ => Order::Asc,
        };
        select.order_by(quoted, order);
    }

    select.limit(limit).offset(offset);

    let inner_sql = select.to_string(PostgresQueryBuilder);

    // Wrap in json_agg() so we get a single JSON array result.
    let wrapped_sql = format!(
        "SELECT COALESCE(json_agg(\"_q\"), '[]'::json) FROM ({}) AS \"_q\"",
        inner_sql,
    );

    // -- Execute and bind ------------------------------------------------
    let mut q = sqlx::query_as::<_, (serde_json::Value,)>(&wrapped_sql);
    for val in &bind_values {
        q = crate::bind_json_value!(q, val);
    }

    let (result,): (serde_json::Value,) = q.fetch_one(pool).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Collection items query failed: {}", e),
        }
    })?;

    let items = match result {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    if items.is_empty() || display_fields.is_empty() {
        return augment_items_with_inline_parents(pool, &items, &collection, collection_name).await;
    }

    // Augment items with display values (internal call, no permission checking)
    let augmented = augment_items_with_display_values(pool, &items, &display_fields, collection_name, None).await?;
    augment_items_with_inline_parents(pool, &augmented, &collection, collection_name).await
}

/// Augment items with inline parent field data for relationship fields
/// that have `inline_parent_fields` configured.
pub async fn augment_items_with_inline_parents(
    pool: &Pool,
    items: &[serde_json::Value],
    collection: &CollectionDefinition,
    _collection_name: &str,
) -> Result<Vec<serde_json::Value>, AppError> {
    if items.is_empty() {
        return Ok(items.to_vec());
    }

    let inline_fields: Vec<&FieldDefinition> = collection.fields.iter()
        .filter(|f| {
            f.field_type == FieldType::Relationship
                && f.inline_parent_fields.as_ref().is_some_and(|v| !v.is_empty())
        })
        .collect();

    if inline_fields.is_empty() {
        return Ok(items.to_vec());
    }

    let mut augmented: Vec<serde_json::Value> = items.to_vec();

    for field in &inline_fields {
        let target = field.related_collection.as_ref().expect("related_collection required");
        let parent_fields = field.inline_parent_fields.as_ref().expect("checked above");

        let uuids: Vec<String> = items.iter()
            .filter_map(|item| item.get(&field.name))
            .filter_map(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        if uuids.is_empty() {
            continue;
        }

        let parent_cols: Vec<String> = parent_fields.iter()
            .map(|f| format!("\"{}\"", f))
            .collect();

        // Use parameterized bindings instead of string interpolation to
        // prevent SQL injection via crafted UUID field values.
        let placeholders: Vec<String> = (1..=uuids.len())
            .map(|i| format!("${}::uuid", i))
            .collect();

        let sql = format!(
            "SELECT row_to_json(\"_p\".*) FROM (SELECT {} FROM \"{}\" WHERE \"id\" IN ({})) AS \"_p\"",
            parent_cols.join(", "),
            target,
            placeholders.join(", ")
        );

        let mut query = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
        for uuid_str in &uuids {
            query = query.bind(uuid_str.as_str());
        }
        let rows: Vec<(serde_json::Value,)> = query.fetch_all(pool).await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Inline parent query failed: {}", e),
            })?;

        let parent_objects: Vec<serde_json::Value> = rows.into_iter().map(|r| r.0).collect();

        for (item, parent_obj) in augmented.iter_mut().zip(parent_objects.iter()) {
            if let Some(obj) = item.as_object_mut() {
                obj.insert(format!("{}__inline_parent", field.name), parent_obj.clone());
            }
        }
    }

    Ok(augmented)
}

/// Resolve File field values (UUID[]) to full file metadata objects.
/// Replaces raw UUID arrays with file metadata objects including download URLs.
pub async fn augment_items_with_file_metadata(
    pool: &Pool,
    items: &[serde_json::Value],
    file_fields: &[&FieldDefinition],
) -> Result<Vec<serde_json::Value>, AppError> {
    if items.is_empty() || file_fields.is_empty() {
        return Ok(items.to_vec());
    }

    // Collect all file UUIDs across all items and fields
    let mut all_file_ids: Vec<String> = Vec::new();
    for item in items {
        for field in file_fields {
            if let Some(serde_json::Value::Array(ids)) = item.get(&field.name) {
                for id_val in ids {
                    if let Some(s) = id_val.as_str() {
                        if !all_file_ids.contains(&s.to_string()) {
                            all_file_ids.push(s.to_string());
                        }
                    }
                }
            }
        }
    }

    if all_file_ids.is_empty() {
        return Ok(items.to_vec());
    }

    // Batch query file_metadata for all referenced UUIDs
    let placeholders: Vec<String> = (1..=all_file_ids.len())
        .map(|i| format!("${}::uuid", i))
        .collect();

    let sql = format!(
        r#"SELECT id, filename, mime_type, size_bytes, storage_provider, storage_path, sha256, alt_text, uploaded_by, created_at, updated_at
           FROM file_metadata WHERE id IN ({})"#,
        placeholders.join(", ")
    );

    let mut query = sqlx::query_as::<_, (
        uuid::Uuid, String, String, i64, String, String, Option<String>, Option<String>,
        Option<uuid::Uuid>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(&sql);
    for id_str in &all_file_ids {
        query = query.bind(id_str.as_str());
    }
    let rows = query.fetch_all(pool).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("File metadata query failed: {}", e),
        }
    })?;

    // Build a map: file UUID -> metadata JSON object
    let mut file_map: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    for row in rows {
        file_map.insert(row.0.to_string(), serde_json::json!({
            "id": row.0,
            "filename": row.1,
            "mime_type": row.2,
            "size_bytes": row.3,
            "storage_provider": row.4,
            "storage_path": row.5,
            "sha256": row.6,
            "alt_text": row.7,
            "uploaded_by": row.8,
            "created_at": row.9,
            "updated_at": row.10,
            "download_url": format!("/api/files/{}/download", row.0),
        }));
    }

    // Replace raw UUID arrays with file metadata objects
    let mut augmented: Vec<serde_json::Value> = items.to_vec();
    for item in augmented.iter_mut() {
        for field in file_fields {
            if let Some(serde_json::Value::Array(ids)) = item.get(&field.name) {
                let metadata_objects: Vec<serde_json::Value> = ids.iter()
                    .filter_map(|id_val| id_val.as_str().and_then(|s| file_map.get(s)))
                    .cloned()
                    .collect();
                if !metadata_objects.is_empty() {
                    item.as_object_mut()
                        .map(|obj| obj.insert(field.name.clone(), serde_json::Value::Array(metadata_objects)));
                }
            }
        }
    }

    Ok(augmented)
}

/// Post-process items to add display values for relationship fields.
///
/// When `permissions_map` is `Some`, each display field is checked:
/// - If the caller has "read" permission on the related collection → resolve display values
/// - If the caller has no permission → skip this field (no display values)
/// When `permissions_map` is `None` (direct API call, no auth) → all display values are resolved.
pub async fn augment_items_with_display_values(
    pool: &Pool,
    items: &[serde_json::Value],
    display_fields: &[&FieldDefinition],
    _collection_name: &str,
    permissions_map: Option<&std::collections::HashMap<String, Vec<PolicyPermission>>>,
) -> Result<Vec<serde_json::Value>, AppError> {
    if items.is_empty() {
        return Ok(items.to_vec());
    }

    let mut augmented: Vec<serde_json::Value> = items.to_vec();

    for field in display_fields {
        let target = field.related_collection.as_ref().expect("related_collection required");

        // Permission check: skip if caller has no read permission on the related collection
        let has_perm = permissions_map
            .map(|m| m.get(target.as_str())
                .map(|perms| crate::services::permissions::authorize_action(perms, "read"))
                .unwrap_or(false))
            .unwrap_or(true); // true when no permission checking (direct API call)
        if !has_perm {
            continue;
        }

        let display_col = field.display_field.as_ref().expect("display_field required");

        // Extract all UUID values for this field
        let uuids: Vec<String> = items.iter()
            .filter_map(|item| item.get(&field.name))
            .filter_map(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        if uuids.is_empty() {
            continue;
        }

        // Use parameterized bindings instead of string interpolation to
        // prevent SQL injection via crafted UUID field values.
        let placeholders: Vec<String> = (1..=uuids.len())
            .map(|i| format!("${}::uuid", i))
            .collect();

        // Batch query: fetch display values for all referenced UUIDs
        let sql = format!(
            "SELECT \"id\"::text, \"{}\" FROM \"{}\" WHERE \"id\" IN ({})",
            display_col, target, placeholders.join(", ")
        );

        let mut query = sqlx::query_as::<_, (String, Option<String>)>(&sql);
        for uuid_str in &uuids {
            query = query.bind(uuid_str.as_str());
        }
        let rows: Vec<(String, Option<String>)> = query.fetch_all(pool).await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Display value query failed: {}", e),
            })?;

        let display_map: std::collections::HashMap<String, String> = rows.into_iter()
            .filter_map(|(id, val)| val.map(|v| (id, v)))
            .collect();

        for item in augmented.iter_mut() {
            if let Some(val) = item.get(&field.name).and_then(|v| v.as_str()) {
                if let Some(display_val) = display_map.get(val) {
                    item.as_object_mut()
                        .map(|obj| obj.insert(format!("{}__display_value", field.name), serde_json::Value::String(display_val.clone())));
                }
            }
        }
    }

    Ok(augmented)
}

/// Count items matching an optional filter (same filter format as query_items).
pub async fn count_items(
    pool: &Pool,
    collection_name: &str,
    filter: Option<serde_json::Value>,
) -> Result<i64, AppError> {
    let collection = resolve_collection_fields(pool, collection_name).await?;

    let col_type_map: std::collections::HashMap<&str, &FieldType> = collection.fields.iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();

    let mut bind_values: Vec<serde_json::Value> = Vec::new();

    let filter_clauses: Vec<String> = if let Some(ref filter_json) = filter {
        if let Some(obj) = filter_json.as_object() {
            obj.iter().map(|(key, val)| {
                let idx = bind_values.len() as u32 + 1;
                let quoted_key = super::quote_identifier(key);
                let val_str = val.as_str().unwrap_or("");
                if let Some(search_term) = val_str.strip_prefix("contains:") {
                    bind_values.push(serde_json::Value::String(format!("%{}%", search_term)));
                    format!("{} LIKE ${}", quoted_key, idx)
                } else {
                    let placeholder = match col_type_map.get(key.as_str()) {
                        Some(FieldType::Uuid) | Some(FieldType::Relationship) => format!("${}::uuid", idx),
                        Some(FieldType::Datetime) => format!("${}::timestamptz", idx),
                        _ => format!("${}", idx),
                    };
                    bind_values.push(val.clone());
                    format!("{} = {}", quoted_key, placeholder)
                }
            }).collect()
        } else { vec![] }
    } else { vec![] };

    let where_clause = if filter_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", filter_clauses.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM \"{}\"{}", collection_name, where_clause);

    let mut q = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values {
        q = crate::bind_json_value!(q, val);
    }

    let count = q.fetch_one(pool).await.map_err(|e| AppError::DatabaseError {
        details: format!("Collection count query failed: {}", e),
    })?;

    Ok(count)
}

// ---------------------------------------------------------------------------
// Grouped query (Phase 39 — Kanban view)
// ---------------------------------------------------------------------------

/// POST /api/collections/:name/items/grouped
///
/// Returns collection items grouped by the specified field, with each group
/// containing its distinct value, item count, and the items themselves.
/// Supports optional filtering, within-group sorting, and pagination of groups.
///
/// Used by the Kanban frontend to render columns per distinct group-by value.
/// Null group-by values are preserved (converted via a sentinel in SQL).
pub async fn grouped_query_items(
    pool: &Pool,
    collection_name: &str,
    request: GroupedQueryRequest,
    extra_permissions: &[crate::services::permissions::PolicyPermission],
) -> Result<GroupedQueryResponse, AppError> {
    // Resolve schema to validate collection exists
    let _collection = resolve_collection_fields(pool, collection_name).await?;

    // Build col_name -> field_type map for UUID cast detection in filter bindings
    let col_type_map: std::collections::HashMap<&str, &FieldType> = _collection.fields.iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();

    let limit = request.limit.unwrap_or(50);
    let offset = request.offset.unwrap_or(0);
    let group_by_quoted = super::quote_identifier(&request.group_by);

    let mut bind_values: Vec<serde_json::Value> = Vec::new();

    // Build user filter WHERE clause first
    let mut where_clause = match request.filter {
        Some(ref filter_cond) => {
            let clause = compile_filter(filter_cond, &col_type_map, &mut bind_values)?;
            if clause.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", clause)
            }
        }
        None => String::new(),
    };

    // Inject permission filter at the correct offset (after user filter binds)
    if !extra_permissions.is_empty() {
        let (perm_clause, perm_binds) =
            crate::services::permissions::build_filter_clause_with_offset(
                extra_permissions, bind_values.len() as usize, None, Some(&_collection),
            );
        if !perm_clause.is_empty() {
            if where_clause.is_empty() {
                where_clause = format!(" WHERE {}", perm_clause);
            } else {
                where_clause = format!("{} AND ({})", where_clause, perm_clause);
            }
            bind_values.extend(perm_binds);
        }
    }

    // Build sort clause for within-group ordering
    let order_clause = if let Some(ref sort_fields) = request.sort {
        if !sort_fields.is_empty() {
            let sorts: Vec<String> = sort_fields.iter().map(|s| {
                let quoted = super::quote_identifier(&s.field);
                let dir = if s.order.to_lowercase() == "desc" { "DESC" } else { "ASC" };
                format!("{} {}", quoted, dir)
            }).collect();
            format!(" ORDER BY {}", sorts.join(", "))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Build visible column list (excluding hidden fields)
    let visible_fields: Vec<&FieldDefinition> = _collection.fields.iter().filter(|f| !f.hidden).collect();
    let mut all_cols: Vec<String> = visible_fields.iter()
        .map(|f| format!(r#""{0}"."{1}""#, collection_name, f.name))
        .collect();
    for col in &["id", "created_at", "updated_at"] {
        all_cols.push(format!(r#""{0}"."{1}""#, collection_name, col));
    }
    let col_list = all_cols.join(", ");

    // Main grouped query: SELECT group_by_field, count, json_agg of items
    // Uses COALESCE with a sentinel string to preserve NULL group values.
    // Uses a subquery to exclude hidden fields from row_to_json.
    let data_sql = format!(
        r#"SELECT COALESCE(json_agg("_grp"), '[]'::json) FROM (
            SELECT
                COALESCE({0}::text, '__gsd_null__') AS group_value,
                CAST(COUNT(*) AS bigint) AS group_count,
                COALESCE(json_agg(row_to_json("_inner".*) {2}), '[]'::json) AS group_items
            FROM (
                SELECT {6} FROM "{1}" {3}
            ) AS "_inner"
            GROUP BY {0}
            ORDER BY group_value
            LIMIT {4} OFFSET {5}
        ) AS "_grp""#,
        group_by_quoted,
        collection_name,
        order_clause,
        where_clause,
        limit,
        offset,
        col_list,
    );

    let mut data_q = sqlx::query_as::<_, (serde_json::Value,)>(&data_sql);
    for val in &bind_values {
        data_q = crate::bind_json_value!(data_q, val);
    }

    let (data_result,): (serde_json::Value,) = data_q.fetch_one(pool).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Grouped query failed: {}", e),
        }
    })?;

    let raw_groups: Vec<serde_json::Value> = match data_result {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    // Convert raw SQL results into GroupResult structs
    let groups: Vec<GroupResult> = raw_groups.iter().map(|g| {
        let group_value = g.get("group_value")
            .and_then(|v| v.as_str())
            .filter(|s| *s != "__gsd_null__")
            .map(|s| s.to_string());

        let count = g.get("group_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let items = g.get("group_items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        GroupResult {
            value: group_value,
            count,
            items,
        }
    }).collect();

    // Count query: total distinct groups
    let count_sql = format!(
        "SELECT COUNT(*) FROM (SELECT 1 FROM \"{0}\" {1} GROUP BY {2}) AS \"_cnt\"",
        collection_name, where_clause, group_by_quoted
    );

    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values {
        count_q = crate::bind_json_value!(count_q, val);
    }

    let total = count_q.fetch_one(pool).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Grouped count query failed: {}", e),
        }
    })?;

    Ok(GroupedQueryResponse { groups, total })
}

/// Create one or more items in a collection within a single transaction.
///
/// Accepts a single item or an array (untagged).  Each item's field names are
/// validated against the collection definition — unknown / reserved names are
/// rejected with `BadRequest` (CRUD-06).  Values are best-effort coerced to
/// match the declared field type.  On success returns the created rows
/// (including server-generated defaults) via `RETURNING row_to_json(...)`.
struct ParsedCreateItem {
    scalar: serde_json::Map<String, serde_json::Value>,
    relational: serde_json::Map<String, serde_json::Value>,
}

/// Create items in the collection.
///
/// Accepts a single item object or an array of items. Supports nested
/// relational creates in the same request:
/// - O2M reverse keys (e.g. `{ contacts: { create: [...] } }`) are processed
///   after the parent row is inserted, within the same transaction.
/// - M:1 nested objects (e.g. `{ author: { name: "Alice" } }`) create the
///   related record first and replace the object with the created UUID.
pub async fn create_items(
    pool: &Pool,
    collection_name: &str,
    body: CreateItemsBody,
) -> Result<Vec<serde_json::Value>, AppError> {
    let collection = resolve_collection_fields(pool, collection_name).await?;

    let items = match body {
        CreateItemsBody::Single(map) => vec![map],
        CreateItemsBody::Multiple(vec) => vec,
    };

    if items.is_empty() {
        return Err(AppError::BadRequest("No items provided".to_string()));
    }

    // Determine if any key is a potential relational field. A key needs
    // relational processing when it isn't a known scalar field on this
    // collection, or when its value is an object/array (O2M body, M:1 nested
    // object, or a parent-owned one_to_many array). If none of those are
    // present, we can skip loading the full collection list.
    let field_names: std::collections::HashSet<&str> =
        collection.fields.iter().map(|f| f.name.as_str()).collect();
    let has_relational_keys = items.iter().any(|item| {
        item.iter().any(|(k, v)| {
            !field_names.contains(k.as_str()) || v.is_object() || v.is_array()
        })
    });

    let all_collections = if has_relational_keys {
        Some(crate::db::collections::list_collections(pool).await?)
    } else {
        None
    };

    // Split each item into scalar fields + O2M relational bodies. M:1 nested
    // objects stay in `scalar` (processed just before the parent insert).
    let mut parsed: Vec<ParsedCreateItem> = Vec::with_capacity(items.len());

    for item in items {
        let keys: Vec<String> = item.keys().cloned().collect();
        if keys.is_empty() {
            return Err(AppError::BadRequest(
                "Each item must have at least one field".to_string(),
            ));
        }

        let mut scalar = item.clone();
        let mut relational: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        if let Some(ref all_cols) = all_collections {
            for key in &keys {
                let is_o2m = match relational_crud::detect_crud_direction(
                    key, &collection.name, &collection, all_cols,
                ) {
                    Ok(CrudDirection::OneToMany { .. }) => {
                        matches!(item.get(key), Some(v) if v.is_object() || v.is_array())
                    }
                    _ => false,
                };
                if is_o2m {
                    if let Some(v) = scalar.remove(key) {
                        relational.insert(key.clone(), v);
                    }
                }
            }
        }
        parsed.push(ParsedCreateItem { scalar, relational });
    }

    // Validate every item's scalar field names and ensure none are empty.
    for item in &parsed {
        let keys: Vec<String> = item.scalar.keys().cloned().collect();
        if keys.is_empty() {
            return Err(AppError::BadRequest(
                "Each item must have at least one field".to_string(),
            ));
        }
        validate_fields_for_write(&keys, &collection)?;
    }

    // Compute the union of all columns across all items.
    let all_cols: Vec<String> = {
        let mut set = std::collections::BTreeSet::new();
        for item in &parsed {
            for key in item.scalar.keys() {
                set.insert(key.clone());
            }
        }
        set.into_iter().collect()
    };

    if all_cols.is_empty() {
        return Err(AppError::BadRequest("No fields provided".to_string()));
    }

    let col_type_map: std::collections::HashMap<&str, &FieldType> = collection.fields.iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();

    let quoted_cols_str = all_cols.iter().map(|c| super::quote_identifier(c)).collect::<Vec<_>>().join(", ");
    let quoted_table = super::quote_identifier(collection_name);

    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;

    // Process M:1 nested objects (e.g. `{ author: { name: "Alice" } }`) before
    // the parent insert — creates the related record and replaces the object
    // with the created UUID. Runs inside the transaction.
    if let Some(ref all_cols) = all_collections {
        let mut scalar_maps: Vec<serde_json::Map<String, serde_json::Value>> =
            parsed.iter().map(|p| p.scalar.clone()).collect();
        relational_crud::process_create_body_for_relational(
            &mut *tx,
            &collection,
            &mut scalar_maps,
            all_cols,
        ).await?;
        for (p, m) in parsed.iter_mut().zip(scalar_maps) {
            p.scalar = m;
        }
    }

    // Insert items in batches of 100 using multi-row INSERT ... VALUES (...), (...) ...
    let batch_size: usize = 100;
    let mut results: Vec<serde_json::Value> = Vec::with_capacity(parsed.len());

    for chunk in parsed.chunks(batch_size) {
        let mut all_bind_values: Vec<serde_json::Value> = Vec::new();
        let mut value_rows: Vec<String> = Vec::with_capacity(chunk.len());

        for item in chunk {
            let mut row_placeholders: Vec<String> = Vec::with_capacity(all_cols.len());
            for col_name in &all_cols {
                let val = item.scalar.get(col_name.as_str()).cloned().unwrap_or(serde_json::Value::Null);
                let field_type = collection.fields.iter()
                    .find(|f| &f.name == col_name.as_str())
                    .map(|f| &f.field_type);
                let coerced_val = match field_type {
                    Some(ft) => coerce_value(val, ft),
                    None => val,
                };
                let idx = all_bind_values.len() + 1;
                match col_type_map.get(col_name.as_str()) {
                    Some(FieldType::Uuid) | Some(FieldType::Relationship) => {
                        row_placeholders.push(format!("${}::uuid", idx));
                    }
                    Some(FieldType::Datetime) => {
                        row_placeholders.push(format!("${}::timestamptz", idx));
                    }
                    Some(FieldType::File) => {
                        row_placeholders.push(format!("${}::uuid[]", idx));
                    }
                    _ => row_placeholders.push(format!("${}", idx)),
                }
                all_bind_values.push(coerced_val);
            }
            value_rows.push(format!("({})", row_placeholders.join(", ")));
        }

        let sql = format!(
            "INSERT INTO {} ({}) VALUES {} RETURNING row_to_json({}.*)",
            quoted_table,
            quoted_cols_str,
            value_rows.join(", "),
            quoted_table,
        );

        let mut q = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
        for val in &all_bind_values {
            q = crate::bind_json_value!(q, val);
        }

        let rows: Vec<(serde_json::Value,)> = q.fetch_all(&mut *tx).await.map_err(|e| {
            AppError::DatabaseError {
                details: format!("Collection items batch insert failed: {}", e),
            }
        })?;
        for (row,) in rows {
            results.push(row);
        }
    }

    // Insert into item_files for File field values
    for (item, result) in parsed.iter().zip(results.iter()) {
        let item_id = result.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if item_id.is_empty() {
            continue;
        }
        for field in &collection.fields {
            if field.field_type != FieldType::File {
                continue;
            }
            if let Some(raw) = item.scalar.get(&field.name) {
                for fid in file_ids_from_value(raw) {
                    sqlx::query(
                        "INSERT INTO item_files (item_id, collection_name, field_name, file_id) VALUES ($1::uuid, $2, $3, $4::uuid) ON CONFLICT DO NOTHING"
                    )
                    .bind(item_id)
                    .bind(collection_name)
                    .bind(&field.name)
                    .bind(fid)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Failed to insert item_files link: {}", e),
                    })?;
                }
            }
        }
    }

    // Process O2M child creates (e.g. `{ contacts: { create: [...] } }`) within
    // the same transaction — parent + children commit atomically.
    for (item, result) in parsed.iter().zip(results.iter()) {
        if item.relational.is_empty() {
            continue;
        }
        let parent_id = match result.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        let all_cols = all_collections.as_deref().unwrap_or(&[]);
        relational_crud::process_o2m_create_body(
            &mut *tx,
            &collection,
            &parent_id,
            &item.relational,
            all_cols,
        ).await?;
    }

    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;

    Ok(results)
}

/// Update items matching a filter.
///
/// The `update` map keys are validated against the collection definition
/// (unknown/reserved field names rejected with `BadRequest`).  The `filter`
/// map keys may refer to any column (including system columns) since they're
/// used for row selection, not writes.
///
/// Returns the count of affected rows.
pub async fn update_items(
    pool: &Pool,
    collection_name: &str,
    body: UpdateItemsBody,
    perm_filter: Option<(String, Vec<serde_json::Value>)>,
) -> Result<u64, AppError> {
    let collection = resolve_collection_fields(pool, collection_name).await?;

    // Build col_name -> field_type map for UUID cast detection.
    // System fields (id, created_at, updated_at) are not in collection.fields
    // but must be handled for WHERE clause casts.
    let mut col_type_map: std::collections::HashMap<&str, &FieldType> = collection.fields.iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();
    col_type_map.insert("id", &FieldType::Uuid);

    // Validate update field keys (these are writes).
    let update_keys: Vec<String> = body.update.keys().cloned().collect();
    if update_keys.is_empty() {
        return Err(AppError::BadRequest("No fields to update".to_string()));
    }
    validate_fields_for_write(&update_keys, &collection)?;

    // Helper: check column type for SQL cast
    let placeholder_cast = |name: &str, idx: u32| -> String {
        match col_type_map.get(name) {
            Some(FieldType::Uuid) | Some(FieldType::Relationship) => format!("${}::uuid", idx),
            Some(FieldType::Datetime) => format!("${}::timestamptz", idx),
            Some(FieldType::File) => format!("${}::uuid[]", idx),
            _ => format!("${}", idx),
        }
    };

    // Collect all bind values in order (SET values first, then WHERE values).
    let mut all_bind_values: Vec<serde_json::Value> = Vec::new();

    // -- SET clauses -------------------------------------------------------
    // UUID-typed columns get $N::uuid cast so string values work.
    // Datetime columns get $N::timestamptz cast so ISO strings work.
    let mut set_clauses: Vec<String> = Vec::new();
    for key in &update_keys {
        let quoted = super::quote_identifier(key);
        let idx = (all_bind_values.len() as u32) + 1;
        let placeholder = placeholder_cast(key, idx);
        set_clauses.push(format!("{} = {}", quoted, placeholder));
        all_bind_values.push(
            body.update
                .get(key.as_str())
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
    }

    // -- WHERE clauses (from filter JSON object) ---------------------------
    // UUID-typed filter columns also get $N::uuid cast.
    let filter_obj = body.filter.as_object().ok_or_else(|| {
        AppError::BadRequest("Filter must be a JSON object".to_string())
    })?;

    // Validate filter keys — reject dot-notation paths (they need JOIN-based
    // resolution and can't be used as simple column names in WHERE).
    for key in filter_obj.keys() {
        if key.contains('.') {
            return Err(AppError::BadRequest(format!(
                "Filter key '{}' contains dot notation, which is not supported for bulk updates. Use the per-item PATCH endpoint instead.",
                key
            )));
        }
    }

    let mut where_clauses: Vec<String> = Vec::new();
    for (key, val) in filter_obj.iter() {
        let quoted = super::quote_identifier(key);
        let idx = (all_bind_values.len() as u32) + 1;
        let placeholder = placeholder_cast(key, idx);
        where_clauses.push(format!("{} = {}", quoted, placeholder));
        all_bind_values.push(val.clone());
    }

    let quoted_table = super::quote_identifier(collection_name);

    let where_base = where_clauses.join(" AND ");

    // Append permission filter (IN subquery) if provided — AND-ed with user's filter
    let mut full_where = match &perm_filter {
        Some((perm_sql, _)) if !perm_sql.is_empty() && !where_base.is_empty() => {
            format!("({}) AND ({})", where_base, perm_sql)
        }
        Some((perm_sql, _)) if !perm_sql.is_empty() => perm_sql.clone(),
        _ => where_base.clone(),
    };

    if full_where.is_empty() {
        if perm_filter.is_none() {
            // Admin/bypass with no user filter: update all rows
            // Use a truthy condition that still allows the UPDATE to proceed
            full_where = "TRUE".to_string();
        } else {
            return Err(AppError::BadRequest(
                "Filter must have at least one condition".to_string(),
            ));
        }
    }

    // Check if any File fields are being updated — need to manage item_files
    let file_fields_updated: Vec<&str> = collection.fields.iter()
        .filter(|f| f.field_type == FieldType::File && update_keys.contains(&f.name))
        .map(|f| f.name.as_str())
        .collect();

    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;

    // If File fields are being updated, first query the affected item IDs
    let affected_ids: Vec<String> = if file_fields_updated.is_empty() {
        vec![]
    } else {
        let id_sql = format!(
            "SELECT id::text FROM {} WHERE {}",
            quoted_table, full_where,
        );
        let mut id_q = sqlx::query_scalar::<_, String>(&id_sql);
        for val in &all_bind_values {
            id_q = crate::bind_json_value!(id_q, val);
        }
        if let Some((_, ref perm_binds)) = perm_filter {
            for val in perm_binds {
                id_q = crate::bind_json_value!(id_q, val);
            }
        }
        id_q.fetch_all(&mut *tx).await.map_err(|e| {
            AppError::DatabaseError {
                details: format!("Failed to query item IDs for update: {}", e),
            }
        })?
    };

    let sql = format!(
        "UPDATE {} SET {} WHERE {}",
        quoted_table,
        set_clauses.join(", "),
        full_where,
    );

    let mut q = sqlx::query(&sql);
    for val in &all_bind_values {
        q = crate::bind_json_value!(q, val);
    }
    if let Some((_, ref perm_binds)) = perm_filter {
        for val in perm_binds {
            q = crate::bind_json_value!(q, val);
        }
    }

    let result = q.execute(&mut *tx).await.map_err(|e| AppError::DatabaseError {
        details: format!("Collection items update failed: {}", e),
    })?;

    // Handle item_files for File fields being updated
    if !file_fields_updated.is_empty() {
        for item_id in &affected_ids {
            // Delete old item_files entries for this item + field combination
            for field_name in &file_fields_updated {
                sqlx::query("DELETE FROM item_files WHERE item_id = $1::uuid AND field_name = $2")
                    .bind(item_id)
                    .bind(field_name)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Failed to delete item_files: {}", e),
                    })?;

                // Insert new item_files entries
                if let Some(raw) = body.update.get(*field_name) {
                    for fid in file_ids_from_value(raw) {
                        sqlx::query(
                            "INSERT INTO item_files (item_id, collection_name, field_name, file_id) VALUES ($1::uuid, $2, $3, $4::uuid) ON CONFLICT DO NOTHING"
                        )
                        .bind(item_id)
                        .bind(collection_name)
                        .bind(field_name)
                        .bind(fid)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| AppError::DatabaseError {
                            details: format!("Failed to insert item_files link: {}", e),
                        })?;
                    }
                }
            }
        }
    }

    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;

    Ok(result.rows_affected())
}

/// Delete items by filter or primary-key values.
///
/// - If `pk_values` is provided: `DELETE WHERE id IN ($1, $2, …)`
/// - If `filter` is provided: `DELETE WHERE col1 = $1 AND col2 = $2 …`
/// - Providing **both** or **neither** returns `BadRequest`.
///
/// `perm_filter` is an optional (sql_fragment, bind_values) pair from permission
/// policy enforcement, AND-ed with the user's filter.
///
/// Returns the count and deleted rows.
pub async fn delete_items(
    pool: &Pool,
    collection_name: &str,
    body: DeleteItemsBody,
    perm_filter: Option<(String, Vec<serde_json::Value>)>,
) -> Result<(u64, Vec<serde_json::Value>), AppError> {
    // Resolve schema (validate collection exists).
    let _collection = resolve_collection_fields(pool, collection_name).await?;

    let quoted_table = super::quote_identifier(collection_name);
    let mut bind_values: Vec<serde_json::Value> = Vec::new();

    let where_clause = match (body.filter, body.pk_values) {
        (Some(_), Some(_)) => {
            return Err(AppError::BadRequest(
                "Provide either filter or pk_values, not both".to_string(),
            ));
        }
        (None, None) => {
            return Err(AppError::BadRequest(
                "filter or pk_values required".to_string(),
            ));
        }
        (None, Some(pk_values)) => {
            if pk_values.is_empty() {
                return Err(AppError::BadRequest("No pk_values provided".to_string()));
            }
            let placeholders: Vec<String> =
                (1..=pk_values.len()).map(|i| format!("${}::uuid", i)).collect();
            bind_values = pk_values;
            format!("{} IN ({})", super::quote_identifier("id"), placeholders.join(", "))
        }
        (Some(filter), None) => {
            let obj = filter.as_object().ok_or_else(|| {
                AppError::BadRequest("Filter must be a JSON object".to_string())
            })?;
            if obj.is_empty() {
                return Err(AppError::BadRequest(
                    "Filter must have at least one condition".to_string(),
                ));
            }
            let conditions: Vec<String> = obj
                .iter()
                .map(|(key, val)| {
                    let idx = (bind_values.len() as u32) + 1;
                    bind_values.push(val.clone());
                    format!("{} = ${}", super::quote_identifier(key), idx)
                })
                .collect();
            conditions.join(" AND ")
        }
    };

    // Append permission filter if provided (AND-ed with user's filter)
    let full_where = match &perm_filter {
        Some((perm_sql, _)) if !perm_sql.is_empty() && !where_clause.is_empty() => {
            format!("({}) AND ({})", where_clause, perm_sql)
        }
        Some((perm_sql, _)) if !perm_sql.is_empty() => perm_sql.clone(),
        _ => where_clause,
    };

    if full_where.is_empty() {
        return Err(AppError::BadRequest(
            "filter or pk_values required".to_string(),
        ));
    }

    let sql = format!(
        "DELETE FROM {} WHERE {} RETURNING row_to_json({}) AS deleted_item",
        quoted_table, full_where, quoted_table
    );

    let mut q = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
    for val in &bind_values {
        q = crate::bind_json_value!(q, val);
    }

    // Bind permission filter parameters
    if let Some((_, ref perm_binds)) = perm_filter {
        for val in perm_binds {
            q = crate::bind_json_value!(q, val);
        }
    }

    let rows: Vec<(serde_json::Value,)> = q.fetch_all(pool).await.map_err(|e| AppError::DatabaseError {
        details: format!("Collection items delete failed: {}", e),
    })?;

    let count = rows.len() as u64;
    let items: Vec<serde_json::Value> = rows.into_iter().map(|(v,)| v).collect();

    // Clean up item_files junction table for deleted items
    if !items.is_empty() {
        let item_ids: Vec<String> = items.iter()
            .filter_map(|item| item.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();
        if !item_ids.is_empty() {
            let placeholders: Vec<String> = (1..=item_ids.len()).map(|i| format!("${}", i)).collect();
            let delete_sql = format!(
                "DELETE FROM item_files WHERE item_id IN ({}) AND collection_name = ${}",
                placeholders.join(", "),
                item_ids.len() + 1
            );
            let mut delete_q = sqlx::query(&delete_sql);
            for id in &item_ids {
                delete_q = delete_q.bind(uuid::Uuid::parse_str(id).unwrap_or_default());
            }
            delete_q = delete_q.bind(collection_name);
            let _ = delete_q.execute(pool).await;
        }
    }

    Ok((count, items))
}
