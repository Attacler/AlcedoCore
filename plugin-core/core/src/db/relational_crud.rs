//! Relational CRUD operations for inline create, update, and unlink of related
//! records through parent item API endpoints.
//!
//! 72-relational-crud

use serde_json::{Map, Value};
use sqlx::{PgConnection, PgPool, Postgres};

use crate::db::collections::{CollectionDefinition, FieldType};
use crate::db::filter_compiler::quote;
use crate::error::AppError;

// ---------------------------------------------------------------------------
// Direction detection for CRUD operations
// ---------------------------------------------------------------------------

pub enum CrudDirection {
    ManyToOne { target_collection: String, fk_column: String },
    OneToMany { target_collection: String, fk_column: String },
}

/// Detect whether a body field name represents a M:1 forward or 1:M reverse
/// relationship. Returns the direction + metadata needed for CRUD operations.
pub fn detect_crud_direction(
    segment: &str,
    current_collection: &str,
    current_def: &CollectionDefinition,
    all_collections: &[CollectionDefinition],
) -> Result<CrudDirection, AppError> {
    // 1. Check if the segment matches a Relationship field on the current collection.
    //    Distinguish M:1 forward (many_to_one, one_to_one) from 1:M reverse (one_to_many).
    if let Some(field) = current_def.fields.iter().find(|f| {
        f.name == segment && f.field_type == FieldType::Relationship
    }) {
        if let Some(ref target) = field.related_collection {
            match field.relationship_type.as_deref() {
                Some("one_to_many") => {
                    // O2M reverse field — find the FK column on the child that points back
                    let fk_column = find_reverse_fk_column(all_collections, target, current_collection)?;
                    return Ok(CrudDirection::OneToMany {
                        target_collection: target.clone(),
                        fk_column,
                    });
                }
                _ => {
                    // M:1 forward (many_to_one, one_to_one)
                    return Ok(CrudDirection::ManyToOne {
                        target_collection: target.clone(),
                        fk_column: field.name.clone(),
                    });
                }
            }
        }
    }

    // 2. Check if the segment is a collection name that has a Relationship back to current.
    //    This handles the case where the caller uses the child collection name directly.
    if let Some(target_def) = all_collections.iter().find(|c| c.name == segment) {
        if let Some(reverse_field) = target_def.fields.iter().find(|f| {
            f.field_type == FieldType::Relationship
                && f.related_collection.as_deref() == Some(current_collection)
        }) {
            return Ok(CrudDirection::OneToMany {
                target_collection: target_def.name.clone(),
                fk_column: reverse_field.name.clone(),
            });
        }
    }

    Err(AppError::BadRequest(format!(
        "'{}' is not a relationship field on '{}' and not a related collection",
        segment, current_collection
    )))
}

/// Find the FK column name on `target_collection` that references back to `current_collection`.
fn find_reverse_fk_column(
    all_collections: &[CollectionDefinition],
    target_collection: &str,
    current_collection: &str,
) -> Result<String, AppError> {
    let target_def = all_collections.iter()
        .find(|c| c.name == target_collection)
        .ok_or_else(|| AppError::BadRequest(format!(
            "Target collection '{}' not found", target_collection
        )))?;
    let reverse_field = target_def.fields.iter()
        .find(|f| {
            f.field_type == FieldType::Relationship
                && f.related_collection.as_deref() == Some(current_collection)
        })
        .ok_or_else(|| AppError::BadRequest(format!(
            "No reverse relationship found on '{}' pointing to '{}'",
            target_collection, current_collection
        )))?;
    Ok(reverse_field.name.clone())
}

// ---------------------------------------------------------------------------
// POST body processing — M:1 nested creates (uses pool directly)
// ---------------------------------------------------------------------------

/// Process a CreateItemsBody for relational fields. For each M:1 relationship
/// field whose value is a nested JSON object (without `id`), creates a new
/// record in the related collection and replaces the object with the created
/// UUID.
pub async fn process_create_body_for_relational(
    pool: &PgPool,
    collection: &CollectionDefinition,
    items: &mut [Map<String, Value>],
) -> Result<(), AppError> {
    let all_collections = crate::db::collections::list_collections(pool).await?;

    for item in items.iter_mut() {
        let field_names: Vec<String> = item.keys().cloned().collect();
        for key in &field_names {
            let val = match item.get(key) {
                Some(v) => v.clone(),
                None => continue,
            };

            let dir = match detect_crud_direction(key, &collection.name, collection, &all_collections) {
                Ok(d) => d,
                Err(_) => continue,
            };

            match dir {
                CrudDirection::ManyToOne { ref target_collection, .. } => {
                    if let Value::Object(obj) = &val {
                        if !obj.contains_key("id") {
                            let target_def = get_collection_def(&all_collections, target_collection)?;
                            let created_id = insert_record(pool, target_collection, target_def, obj).await?;
                            item.insert(key.clone(), Value::String(created_id));
                        }
                    }
                }
                CrudDirection::OneToMany { .. } => {}
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// PATCH body processing — transactional nested CRUD
// ---------------------------------------------------------------------------

pub struct RelationalUpdateResult {
    pub scalar_fields: Map<String, Value>,
}

/// Process a PATCH body for relational fields within an existing transaction.
///
/// For each field in the body:
/// - M:1 with nested object (no id) → INSERT into related table, replace with UUID
/// - M:1 with nested object (has id) → UPDATE related table, exclude from parent SET
/// - M:1 with null → include in parent SET with NULL (unlink)
/// - M:1 with UUID string → pass through as scalar
/// - O2M with array → process on child table, exclude from parent SET
/// - O2M with detailed {create, update, delete} → process bulk, exclude from parent SET
/// - O2M with null → unlink all child records, exclude from parent SET
/// - Non-relational fields → pass through as scalar
pub async fn process_update_body_for_relational(
    conn: &mut PgConnection,
    collection: &CollectionDefinition,
    parent_id: &str,
    body: &Map<String, Value>,
    all_collections: &[CollectionDefinition],
) -> Result<RelationalUpdateResult, AppError> {
    let mut scalar_fields = Map::new();

    for (key, val) in body.iter() {
        // Skip inline parent field updates — they're handled by the caller (Phase 75)
        if key.starts_with("__parent__") {
            continue;
        }
        let dir = match detect_crud_direction(key, &collection.name, collection, all_collections) {
            Ok(d) => d,
            Err(_) => {
                scalar_fields.insert(key.clone(), val.clone());
                continue;
            }
        };

        match dir {
            CrudDirection::ManyToOne { ref target_collection, fk_column: _ } => {
                match val {
                    Value::Object(obj) if obj.contains_key("id") => {
                        let target_def = get_collection_def(all_collections, target_collection)?;
                        let record_id = obj.get("id").and_then(|v| v.as_str())
                            .ok_or_else(|| AppError::BadRequest("id must be a string".to_string()))?;
                        let mut update_fields = obj.clone();
                        update_fields.remove("id");
                        if !update_fields.is_empty() {
                            update_record(&mut *conn, target_collection, target_def, record_id, &update_fields).await?;
                        }
                    }
                    Value::Object(obj) => {
                        let target_def = get_collection_def(all_collections, target_collection)?;
                        let created_id = insert_record(&mut *conn, target_collection, target_def, obj).await?;
                        scalar_fields.insert(key.clone(), Value::String(created_id));
                    }
                    Value::Null => {
                        scalar_fields.insert(key.clone(), Value::Null);
                    }
                    _ => {
                        scalar_fields.insert(key.clone(), val.clone());
                    }
                }
            }
            CrudDirection::OneToMany { ref target_collection, ref fk_column } => {
                match val {
                    Value::Array(arr) => {
                        let mut create_items: Vec<Map<String, Value>> = Vec::new();
                        let mut assign_ids: Vec<String> = Vec::new();

                        for elem in arr {
                            match elem {
                                Value::Object(obj) => create_items.push(obj.clone()),
                                Value::String(s) => assign_ids.push(s.clone()),
                                _ => {
                                    return Err(AppError::BadRequest(format!(
                                        "Invalid element in O2M array for '{}': expected object or UUID string", key
                                    )));
                                }
                            }
                        }

                        let target_def = get_collection_def(all_collections, target_collection)?;
                        for item in &create_items {
                            let mut create_body = item.clone();
                            create_body.insert(fk_column.clone(), Value::String(parent_id.to_string()));
                            insert_record(&mut *conn, target_collection, target_def, &create_body).await?;
                        }
                        if !assign_ids.is_empty() {
                            assign_child_records(&mut *conn, target_collection, fk_column, parent_id, &assign_ids).await?;
                        }
                    }
                    Value::Object(details) => {
                        let has_create = details.contains_key("create");
                        let has_update = details.contains_key("update");
                        let has_delete = details.contains_key("delete");

                        if !has_create && !has_update && !has_delete {
                            return Err(AppError::BadRequest(format!(
                                "O2M field '{}' object must contain 'create', 'update', or 'delete' keys", key
                            )));
                        }

                        let target_def = get_collection_def(all_collections, target_collection)?;

                        if let Some(create_arr) = details.get("create").and_then(|v| v.as_array()) {
                            for obj_val in create_arr {
                                if let Some(obj) = obj_val.as_object() {
                                    let mut create_body = obj.clone();
                                    create_body.insert(fk_column.clone(), Value::String(parent_id.to_string()));
                                    insert_record(&mut *conn, target_collection, target_def, &create_body).await?;
                                }
                            }
                        }

                        if let Some(update_arr) = details.get("update").and_then(|v| v.as_array()) {
                            for obj_val in update_arr {
                                if let Some(obj) = obj_val.as_object() {
                                    if let Some(id_str) = obj.get("id").and_then(|v| v.as_str()) {
                                        let mut fields = obj.clone();
                                        fields.remove("id");
                                        if !fields.is_empty() {
                                            update_record(&mut *conn, target_collection, target_def, id_str, &fields).await?;
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(delete_arr) = details.get("delete").and_then(|v| v.as_array()) {
                            let delete_ids: Vec<String> = delete_arr.iter()
                                .filter_map(|e| e.as_str().map(|s| s.to_string()))
                                .collect();
                            if !delete_ids.is_empty() {
                                delete_child_records(&mut *conn, target_collection, fk_column, parent_id, &delete_ids).await?;
                            }
                        }
                    }
                    Value::Null => {
                        unlink_all_child_records(&mut *conn, target_collection, fk_column, parent_id).await?;
                    }
                    _ => {
                        return Err(AppError::BadRequest(format!(
                            "Invalid value type for O2M field '{}': expected object, array, or null", key
                        )));
                    }
                }
            }
        }
    }

    Ok(RelationalUpdateResult { scalar_fields })
}

// ---------------------------------------------------------------------------
// Low-level SQL helpers (generic over Executor)
// ---------------------------------------------------------------------------

async fn insert_record<'e, E>(
    executor: E,
    collection_name: &str,
    target_def: &CollectionDefinition,
    values: &Map<String, Value>,
) -> Result<String, AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let cols: Vec<&String> = values.keys().collect();
    if cols.is_empty() {
        return Err(AppError::BadRequest("Cannot create record with no fields".to_string()));
    }

    let col_type_map: std::collections::HashMap<&str, &FieldType> = target_def.fields.iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();

    let quoted_cols: Vec<String> = cols.iter().map(|c| quote(c.as_str())).collect();
    let placeholders: Vec<String> = cols.iter().enumerate().map(|(i, col_name)| {
        let idx = i + 1;
        match col_type_map.get(col_name.as_str()) {
            Some(FieldType::Uuid) | Some(FieldType::Relationship) => format!("${}::uuid", idx),
            Some(FieldType::Datetime) => format!("${}::timestamptz", idx),
            _ => format!("${}", idx),
        }
    }).collect();

    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) RETURNING id",
        quote(collection_name),
        quoted_cols.join(", "),
        placeholders.join(", "),
    );

    let vals: Vec<Value> = cols.iter()
        .map(|col_name| values.get(col_name.as_str()).cloned().unwrap_or(Value::Null))
        .collect();

    let mut q = sqlx::query_as::<_, (uuid::Uuid,)>(&sql);
    for val in &vals {
        q = crate::bind_json_value!(q, val);
    }

    let (created_id,): (uuid::Uuid,) = q.fetch_one(executor).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Failed to insert into '{}': {}", collection_name, e),
        }
    })?;

    Ok(created_id.to_string())
}

async fn update_record<'e, E>(
    executor: E,
    collection_name: &str,
    target_def: &CollectionDefinition,
    record_id: &str,
    values: &Map<String, Value>,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let cols: Vec<&String> = values.keys().collect();
    if cols.is_empty() {
        return Ok(());
    }

    let col_type_map: std::collections::HashMap<&str, &FieldType> = target_def.fields.iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();

    let mut set_clauses: Vec<String> = Vec::new();
    let vals: Vec<Value> = cols.iter().map(|col_name| {
        let idx = set_clauses.len() as u32 + 1;
        let placeholder = match col_type_map.get(col_name.as_str()) {
            Some(FieldType::Uuid) | Some(FieldType::Relationship) => format!("${}::uuid", idx),
            Some(FieldType::Datetime) => format!("${}::timestamptz", idx),
            _ => format!("${}", idx),
        };
        set_clauses.push(format!("{} = {}", quote(col_name.as_str()), placeholder));
        values.get(col_name.as_str()).cloned().unwrap_or(Value::Null)
    }).collect();

    let id_idx = set_clauses.len() as u32 + 1;
    let sql = format!(
        "UPDATE {} SET {} WHERE id = ${}::uuid",
        quote(collection_name),
        set_clauses.join(", "),
        id_idx,
    );

    let mut q = sqlx::query(&sql);
    for val in &vals {
        q = crate::bind_json_value!(q, val);
    }
    q = q.bind(record_id);

    q.execute(executor).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Failed to update '{}': {}", collection_name, e),
        }
    })?;

    Ok(())
}

async fn assign_child_records<'e, E>(
    executor: E,
    child_collection: &str,
    fk_column: &str,
    parent_id: &str,
    child_ids: &[String],
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    if child_ids.is_empty() {
        return Ok(());
    }

    let placeholders: Vec<String> = (1..=child_ids.len())
        .map(|i| format!("${}::uuid", i))
        .collect();
    let fk_idx = child_ids.len() as u32 + 1;

    let sql = format!(
        "UPDATE {} SET {} = ${}::uuid WHERE id IN ({})",
        quote(child_collection),
        quote(fk_column),
        fk_idx,
        placeholders.join(", "),
    );

    let mut q = sqlx::query(&sql);
    for id in child_ids {
        q = q.bind(id.as_str());
    }
    q = q.bind(parent_id);

    q.execute(executor).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Failed to assign child records: {}", e),
        }
    })?;

    Ok(())
}

async fn delete_child_records<'e, E>(
    executor: E,
    child_collection: &str,
    fk_column: &str,
    parent_id: &str,
    child_ids: &[String],
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    if child_ids.is_empty() {
        return Ok(());
    }

    let placeholders: Vec<String> = (1..=child_ids.len())
        .map(|i| format!("${}::uuid", i))
        .collect();
    let fk_idx = child_ids.len() as u32 + 1;

    let sql = format!(
        "DELETE FROM {} WHERE id IN ({}) AND {} = ${}::uuid",
        quote(child_collection),
        placeholders.join(", "),
        quote(fk_column),
        fk_idx,
    );

    let mut q = sqlx::query(&sql);
    for id in child_ids {
        q = q.bind(id.as_str());
    }
    q = q.bind(parent_id);

    q.execute(executor).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Failed to delete child records: {}", e),
        }
    })?;

    Ok(())
}

async fn unlink_all_child_records<'e, E>(
    executor: E,
    child_collection: &str,
    fk_column: &str,
    parent_id: &str,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let sql = format!(
        "UPDATE {} SET {} = NULL WHERE {} = $1::uuid",
        quote(child_collection),
        quote(fk_column),
        quote(fk_column),
    );

    sqlx::query(&sql)
        .bind(parent_id)
        .execute(executor)
        .await
        .map_err(|e| {
            AppError::DatabaseError {
                details: format!("Failed to unlink child records: {}", e),
            }
        })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_collection_def<'a>(
    all_collections: &'a [CollectionDefinition],
    name: &str,
) -> Result<&'a CollectionDefinition, AppError> {
    all_collections.iter()
        .find(|c| c.name == name)
        .ok_or_else(|| AppError::NotFound(format!("Collection '{}' not found", name)))
}

