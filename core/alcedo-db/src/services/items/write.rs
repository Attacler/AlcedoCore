use serde_json::{Map, Value};
use sqlx::{Postgres, Transaction};

use alcedo_common::error::AppError;

use crate::db::collection_items::CreateItemsBody;
use crate::db::filter_compiler;
use crate::db::filter_condition::FilterCondition;
use crate::db::relational_crud;
use crate::db::Pool;
use crate::services::items::shape::{
    column_cast, coerce_physical_value, qualified_table, validate_fields_for_write_shape,
    ColumnShape, TableShape,
};
use crate::services::permissions::PolicyPermission;

/// Result of a write operation, consumed by the API boundary to emit events.
#[derive(Debug, Clone, Default)]
pub struct WriteOutcome {
    /// Rows returned to the client (created items, updated rows, or deleted old rows).
    pub affected: Vec<Value>,
    /// Old values needed for delete events.
    pub deleted: Vec<Value>,
    /// (old, new) pairs for update events.
    pub pairs: Vec<(Value, Value)>,
    /// Number of rows affected by the write operation.
    pub affected_count: u64,
}

pub async fn execute_create(
    pool: &Pool,
    collection: &str,
    body: CreateItemsBody,
) -> Result<WriteOutcome, AppError> {
    let affected = crate::db::collection_items::create_items(pool, collection, body).await?;
    Ok(WriteOutcome {
        affected,
        ..Default::default()
    })
}

pub async fn execute_update(
    pool: &Pool,
    collection: &str,
    body: crate::db::collection_items::UpdateItemsBody,
    perm_filter: Option<(String, Vec<Value>)>,
) -> Result<WriteOutcome, AppError> {
    let (affected_count, affected) =
        crate::db::collection_items::update_items(pool, collection, body, perm_filter).await?;
    Ok(WriteOutcome {
        affected_count,
        affected,
        ..Default::default()
    })
}

/// Update a single item by id: processes nested relational fields, applies
/// scalar updates, enforces row-level permission filters, syncs file links,
/// and returns the old/new pair for event emission.
pub async fn execute_update_one(
    pool: &Pool,
    collection_name: &str,
    id: &str,
    body: &Map<String, Value>,
    permissions: &[PolicyPermission],
) -> Result<WriteOutcome, AppError> {
    let collection = crate::db::collections::get_collection(pool, collection_name).await?;
    let all_collections = crate::db::collections::list_collections(pool).await?;

    // Separate relational fields and scalar fields.
    let mut scalar_keys: Vec<String> = Vec::new();
    for key in body.keys() {
        let is_relational = relational_crud::detect_crud_direction(
            key,
            &collection.name,
            &collection,
            &all_collections,
        )
        .is_ok();
        if !is_relational {
            scalar_keys.push(key.clone());
        }
    }

    // Validate scalar field names
    if !scalar_keys.is_empty() {
        crate::db::collection_items::validate_fields_for_write(&scalar_keys, &collection)?;
    }

    // Begin transaction for atomic relational CRUD (RCRUD-06)
    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;

    // Process relational fields within the transaction
    let relational_result = relational_crud::process_update_body_for_relational(
        &mut *tx,
        &collection,
        id,
        body,
        &all_collections,
    )
    .await?;

    let scalar_fields = &relational_result.scalar_fields;
    let has_scalar_updates = !scalar_fields.is_empty();

    // Build col_name -> field_type map for UUID cast detection on scalar fields
    let col_type_map: std::collections::HashMap<&str, &crate::db::collections::FieldType> =
        collection
            .fields
            .iter()
            .map(|f| (f.name.as_str(), &f.field_type))
            .collect();

    // --- Fetch old item before update for diff computation ---
    let quoted_table_filter = crate::db::filter_compiler::quote(collection_name);
    let quoted_id_filter = crate::db::filter_compiler::quote("id");
    let old_sql = format!(
        "SELECT row_to_json({}.*) FROM {} WHERE {} = $1::uuid",
        quoted_table_filter, quoted_table_filter, quoted_id_filter,
    );
    let (old_item,): (Value,) = sqlx::query_as(&old_sql)
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to fetch old item before update: {}", e),
        })?;

    // Enforce row-level permission filter on updates — check that the item
    // matches at least one update permission's filter. Without this, users
    // could update items that their policy wouldn't normally allow them to see.
    let update_perms: Vec<PolicyPermission> = permissions
        .iter()
        .filter(|p| p.action == "update")
        .cloned()
        .collect();
    if !update_perms.is_empty() {
        let matching =
            crate::services::permissions::item_matches_any_filter(&update_perms, &old_item);
        if matching.is_empty() {
            // item_matches_any_filter fails for dot-notation filters
            // (e.g. order_assignments.user) because the item JSON doesn't
            // contain joined fields. Fall back to a SQL EXISTS check using
            // build_filter_clause_with_joins which resolves dot-notation.
            let all_cols = crate::db::collections::list_collections(pool).await?;

            let (perm_where, perm_binds, join_clauses) =
                crate::services::permissions::build_filter_clause_with_joins(
                    &update_perms,
                    1, // start_idx=1 (reserve $1 for item_id)
                    None,
                    collection_name,
                    &collection,
                    &all_cols,
                );
            if !perm_where.is_empty() {
                // Build: SELECT 1 FROM orders [JOINs] WHERE filter_conditions AND orders.id = $1
                let join_sql = join_clauses.join(" ");
                let check_sql = format!(
                    "SELECT EXISTS(SELECT 1 FROM {} {} WHERE {} AND {}.{} = $1::uuid)",
                    crate::db::filter_compiler::quote(collection_name),
                    join_sql,
                    perm_where,
                    crate::db::filter_compiler::quote(collection_name),
                    crate::db::filter_compiler::quote("id"),
                );
                let mut query = sqlx::query_scalar::<_, bool>(&check_sql);
                query = query.bind(id);
                for val in &perm_binds {
                    query = crate::bind_json_value!(query, val);
                }
                let exists = query
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Permission filter check failed: {}", e),
                    })?
                    .unwrap_or(false);
                if !exists {
                    return Err(AppError::Forbidden(
                        "You do not have permission to update this item".to_string(),
                    ));
                }
            } else {
                return Err(AppError::Forbidden(
                    "You do not have permission to update this item".to_string(),
                ));
            }
        }
    }
    // --- End old value fetch ---

    let quoted_table = crate::db::filter_compiler::quote(collection_name);
    let quoted_id = crate::db::filter_compiler::quote("id");

    let updated: Value = if has_scalar_updates {
        // ---- Build UPDATE SET clauses from scalar fields only -------------
        let mut bind_values: Vec<Value> = Vec::new();
        let mut set_clauses: Vec<String> = Vec::new();

        let scalar_keys_list: Vec<String> = scalar_fields.keys().cloned().collect();
        for key in &scalar_keys_list {
            let q = crate::db::filter_compiler::quote(key);
            let idx = bind_values.len() as u32 + 1;
            let field_type = col_type_map.get(key.as_str());
            let placeholder = match field_type {
                Some(crate::db::collections::FieldType::Uuid)
                | Some(crate::db::collections::FieldType::Relationship) => {
                    format!("${}::uuid", idx)
                }
                Some(crate::db::collections::FieldType::Datetime) => {
                    format!("${}::timestamptz", idx)
                }
                Some(crate::db::collections::FieldType::File) => {
                    format!("${}::uuid[]", idx)
                }
                _ => format!("${}", idx),
            };
            set_clauses.push(format!("{} = {}", q, placeholder));
            let raw_value = scalar_fields
                .get(key.as_str())
                .cloned()
                .unwrap_or(Value::Null);
            let value = if let Some(crate::db::collections::FieldType::File) = field_type {
                crate::db::collection_items::coerce_value(
                    raw_value,
                    &crate::db::collections::FieldType::File,
                )
            } else {
                raw_value
            };
            bind_values.push(value);
        }

        let id_idx = bind_values.len() as u32 + 1;

        let sql = format!(
            "UPDATE {} SET {} WHERE {} = ${}::uuid RETURNING row_to_json({}.*)",
            quoted_table,
            set_clauses.join(", "),
            quoted_id,
            id_idx,
            quoted_table,
        );

        let mut q = sqlx::query_as::<_, (Value,)>(&sql);
        for val in &bind_values {
            q = crate::bind_json_value!(q, val);
        }
        q = q.bind(id);

        let (row,): (Value,) = q
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Collection item update failed: {}", e),
            })?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Item '{}' not found in collection '{}'",
                    id, collection_name
                ))
            })?;
        row
    } else {
        old_item.clone()
    };

    // Sync item_files for File fields being updated
    let file_fields: Vec<&crate::db::collections::FieldDefinition> = collection
        .fields
        .iter()
        .filter(|f| {
            f.field_type == crate::db::collections::FieldType::File && body.contains_key(&f.name)
        })
        .collect();
    if !file_fields.is_empty() {
        for field_def in file_fields {
            sqlx::query(
                "DELETE FROM alcedocore_item_files WHERE item_id = $1::uuid AND field_name = $2",
            )
            .bind(id)
            .bind(&field_def.name)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to delete item_files: {}", e),
            })?;

            if let Some(raw) = body.get(&field_def.name) {
                for fid in crate::db::collection_items::file_ids_from_value(raw) {
                    sqlx::query(
                        "INSERT INTO alcedocore_item_files (item_id, collection_name, field_name, file_id) VALUES ($1::uuid, $2, $3, $4::uuid) ON CONFLICT DO NOTHING"
                    )
                    .bind(id)
                    .bind(collection_name)
                    .bind(&field_def.name)
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

    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;

    Ok(WriteOutcome {
        affected: vec![updated.clone()],
        pairs: vec![(old_item, updated)],
        ..Default::default()
    })
}

pub async fn execute_delete(
    pool: &Pool,
    collection: &str,
    body: crate::db::collection_items::DeleteItemsBody,
    perm_filter: Option<(String, Vec<Value>)>,
) -> Result<WriteOutcome, AppError> {
    let (affected_count, deleted) =
        crate::db::collection_items::delete_items(pool, collection, body, perm_filter).await?;
    Ok(WriteOutcome {
        affected_count,
        deleted,
        ..Default::default()
    })
}

/// Insert one row into a physical table inside a caller-owned transaction.
pub async fn execute_create_one_for_table_tx(
    tx: &mut Transaction<'_, Postgres>,
    shape: &TableShape,
    item: Map<String, Value>,
) -> Result<Value, AppError> {
    let keys: Vec<String> = item.keys().cloned().collect();
    validate_fields_for_write_shape(&keys, shape)?;

    let table = qualified_table(&shape.schema, &shape.name);
    let mut columns: Vec<&ColumnShape> = Vec::new();
    let mut bind_values: Vec<Value> = Vec::new();
    for (field, value) in &item {
        let column = shape
            .columns
            .iter()
            .find(|c| c.name == *field)
            .ok_or_else(|| AppError::BadRequest(format!("Unknown field: '{}'", field)))?;
        columns.push(column);
        bind_values.push(coerce_physical_value(value.clone(), column));
    }

    let insert_sql = if columns.is_empty() {
        format!("INSERT INTO {} DEFAULT VALUES", table)
    } else {
        let column_list = columns
            .iter()
            .map(|c| filter_compiler::quote(&c.name))
            .collect::<Vec<String>>()
            .join(", ");
        let placeholders = columns
            .iter()
            .enumerate()
            .map(|(idx, c)| format!("${}{}", idx + 1, column_cast(c)))
            .collect::<Vec<String>>()
            .join(", ");
        format!("INSERT INTO {} ({}) VALUES ({})", table, column_list, placeholders)
    };

    let sql = format!("{} RETURNING row_to_json({}.*)", insert_sql, table);
    let mut q = sqlx::query_as::<_, (Value,)>(&sql);
    for value in &bind_values {
        q = crate::bind_json_value!(q, value);
    }
    q.fetch_one(&mut **tx)
        .await
        .map(|(row,)| row)
        .map_err(|e| AppError::DatabaseError {
            details: format!("Table insert failed: {}", e),
        })
}

/// Insert rows into a physical (non-collection) table addressed by a
/// [`TableShape`].
///
/// Only the columns present on each item are written, so an omitted serial
/// primary key still auto-increments. Values are coerced from the shape's
/// column types and bound as parameters; the whole batch runs in one
/// transaction so any failure rolls back every row.
pub async fn execute_create_for_table(
    pool: &Pool,
    shape: &TableShape,
    items: Vec<Map<String, Value>>,
) -> Result<WriteOutcome, AppError> {
    if items.is_empty() {
        return Err(AppError::BadRequest(
            "No items provided for table insert".to_string(),
        ));
    }
    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    let mut affected = Vec::with_capacity(items.len());
    for item in items {
        let row = execute_create_one_for_table_tx(&mut tx, shape, item).await?;
        affected.push(row);
    }
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    Ok(WriteOutcome {
        affected,
        ..Default::default()
    })
}

/// ON CONFLICT behaviour for physical-table inserts.
#[derive(Debug, Clone, Copy)]
pub enum ConflictPolicy {
    /// `ON CONFLICT (<cols>) DO NOTHING`. A conflicting insert is skipped and
    /// the `_tx` core returns `Ok(None)` instead of erroring, so duplicate
    /// assignments silently no-op (idempotent).
    DoNothing,
    /// `ON CONFLICT (<cols>) DO UPDATE SET <provided col> = EXCLUDED.<col>`
    /// (only the columns present on the item), plus `updated_at = NOW()` when
    /// the table has an `updated_at` column. Omitted columns are preserved,
    /// giving the same semantics as the settings `COALESCE` upsert.
    Upsert,
}

/// Insert one row into a physical table with an explicit `ON CONFLICT`
/// behaviour, inside a caller-owned transaction.
///
/// Returns `Ok(None)` when a `DoNothing` conflict skipped the insert.
pub async fn execute_insert_for_table_with_conflict_tx(
    tx: &mut Transaction<'_, Postgres>,
    shape: &TableShape,
    conflict_columns: &[&str],
    policy: ConflictPolicy,
    item: Map<String, Value>,
) -> Result<Option<Value>, AppError> {
    if item.is_empty() {
        return Err(AppError::BadRequest(
            "No fields provided for table insert".to_string(),
        ));
    }
    let keys: Vec<String> = item.keys().cloned().collect();
    validate_fields_for_write_shape(&keys, shape)?;

    let table = qualified_table(&shape.schema, &shape.name);
    let mut columns: Vec<&ColumnShape> = Vec::new();
    let mut bind_values: Vec<Value> = Vec::new();
    for (field, value) in &item {
        let column = shape
            .columns
            .iter()
            .find(|c| c.name == *field)
            .ok_or_else(|| AppError::BadRequest(format!("Unknown field: '{}'", field)))?;
        columns.push(column);
        bind_values.push(coerce_physical_value(value.clone(), column));
    }

    let column_list = columns
        .iter()
        .map(|c| filter_compiler::quote(&c.name))
        .collect::<Vec<String>>()
        .join(", ");
    let placeholders = columns
        .iter()
        .enumerate()
        .map(|(idx, c)| format!("${}{}", idx + 1, column_cast(c)))
        .collect::<Vec<String>>()
        .join(", ");
    let conflict = conflict_columns
        .iter()
        .map(|c| filter_compiler::quote(c))
        .collect::<Vec<String>>()
        .join(", ");

    let suffix = match policy {
        ConflictPolicy::DoNothing => format!(" ON CONFLICT ({}) DO NOTHING", conflict),
        ConflictPolicy::Upsert => {
            let mut sets: Vec<String> = columns
                .iter()
                .map(|c| {
                    format!(
                        "{} = EXCLUDED.{}",
                        filter_compiler::quote(&c.name),
                        filter_compiler::quote(&c.name)
                    )
                })
                .collect();
            if shape.columns.iter().any(|c| c.name == "updated_at")
                && !item.contains_key("updated_at")
            {
                sets.push("updated_at = NOW()".to_string());
            }
            format!(" ON CONFLICT ({}) DO UPDATE SET {}", conflict, sets.join(", "))
        }
    };

    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}){} RETURNING row_to_json({}.*)",
        table, column_list, placeholders, suffix, table,
    );
    let mut q = sqlx::query_as::<_, (Value,)>(&sql);
    for value in &bind_values {
        q = crate::bind_json_value!(q, value);
    }
    match policy {
        ConflictPolicy::DoNothing => q
            .fetch_optional(&mut **tx)
            .await
            .map(|row| row.map(|(row,)| row))
            .map_err(|e| AppError::DatabaseError {
                details: format!("Table conflict insert failed: {}", e),
            }),
        ConflictPolicy::Upsert => q
            .fetch_one(&mut **tx)
            .await
            .map(|(row,)| Some(row))
            .map_err(|e| AppError::DatabaseError {
                details: format!("Table conflict insert failed: {}", e),
            }),
    }
}

/// Insert rows into a physical (non-collection) table with an explicit
/// `ON CONFLICT` behaviour, run in a new transaction. Each item is inserted
/// with the same conflict columns and policy.
///
/// `affected`/`affected_count` exclude rows skipped by a `DoNothing` conflict.
pub async fn execute_insert_for_table_with_conflict(
    pool: &Pool,
    shape: &TableShape,
    conflict_columns: &[&str],
    policy: ConflictPolicy,
    items: Vec<Map<String, Value>>,
) -> Result<WriteOutcome, AppError> {
    if items.is_empty() {
        return Err(AppError::BadRequest(
            "No items provided for table insert".to_string(),
        ));
    }
    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    let mut affected = Vec::with_capacity(items.len());
    for item in items {
        if let Some(row) =
            execute_insert_for_table_with_conflict_tx(&mut tx, shape, conflict_columns, policy, item)
                .await?
        {
            affected.push(row);
        }
    }
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    Ok(WriteOutcome {
        affected_count: affected.len() as u64,
        affected,
        ..Default::default()
    })
}

/// Append `updated_at = NOW()` to the SET list when the table has the column
/// and the caller did not provide it (server-managed timestamp convention).
fn auto_updated_at(shape: &TableShape, body: &Map<String, Value>, set_clauses: &mut Vec<String>) {
    if body.contains_key("updated_at") {
        return;
    }
    if shape.columns.iter().any(|c| c.name == "updated_at") {
        set_clauses.push("updated_at = NOW()".to_string());
    }
}

/// Filter→SQL bulk update on a physical table. `filter` is required (updating
/// every row without a predicate is rejected). `perm_filter` is an optional
/// pre-compiled `(sql_fragment, binds)` ANDed into the WHERE. NOTE: for Phase 5
/// its binds are NOT renumbered — callers must pass `None` or a
/// placeholder-free fragment; renumbering is deferred. `updated_at` is
/// auto-refreshed when present.
pub async fn execute_bulk_update_for_table_tx(
    tx: &mut Transaction<'_, Postgres>,
    shape: &TableShape,
    filter: FilterCondition,
    update: &Map<String, Value>,
    perm_filter: Option<(String, Vec<Value>)>,
) -> Result<WriteOutcome, AppError> {
    if update.is_empty() {
        return Err(AppError::BadRequest("No fields to update".to_string()));
    }
    let keys: Vec<String> = update.keys().cloned().collect();
    validate_fields_for_write_shape(&keys, shape)?;

    let table = qualified_table(&shape.schema, &shape.name);
    let col_type_map = shape.col_type_map_ref();
    let mut bind_values: Vec<Value> = Vec::new();
    let clause = filter_compiler::compile_filter(&filter, &col_type_map, &mut bind_values)?;

    let mut set_clauses: Vec<String> = Vec::new();
    for (key, value) in update {
        let column = shape
            .columns
            .iter()
            .find(|c| c.name == *key)
            .ok_or_else(|| AppError::BadRequest(format!("Unknown field: '{}'", key)))?;
        let idx = bind_values.len() + 1;
        set_clauses.push(format!(
            "{} = ${}{}",
            filter_compiler::quote(key),
            idx,
            column_cast(column)
        ));
        bind_values.push(coerce_physical_value(value.clone(), column));
    }
    auto_updated_at(shape, update, &mut set_clauses);

    let mut where_parts = vec![clause];
    if let Some((perm_sql, _perm_binds)) = perm_filter {
        if !perm_sql.is_empty() {
            where_parts.push(perm_sql);
        }
    }
    let where_clause = where_parts.join(" AND ");
    let sql = format!(
        "UPDATE {} SET {} WHERE {} RETURNING row_to_json({}.*)",
        table,
        set_clauses.join(", "),
        where_clause,
        table,
    );

    let mut q = sqlx::query_as::<_, (Value,)>(&sql);
    for val in &bind_values {
        q = crate::bind_json_value!(q, val);
    }
    let rows: Vec<(Value,)> = q.fetch_all(&mut **tx).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Table bulk update failed: {}", e),
        }
    })?;
    let rows: Vec<Value> = rows.into_iter().map(|(row,)| row).collect();
    Ok(WriteOutcome {
        affected_count: rows.len() as u64,
        affected: rows,
        ..Default::default()
    })
}

/// Filter→SQL bulk update on a physical table, run in a new transaction.
///
/// `perm_filter` is an optional pre-compiled `(sql_fragment, binds)` ANDed into
/// the WHERE. NOTE: for Phase 5 its binds are NOT renumbered — callers must
/// pass `None` or a placeholder-free fragment; renumbering is deferred.
pub async fn execute_bulk_update_for_table(
    pool: &Pool,
    shape: &TableShape,
    filter: FilterCondition,
    update: &Map<String, Value>,
    perm_filter: Option<(String, Vec<Value>)>,
) -> Result<WriteOutcome, AppError> {
    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    let outcome = execute_bulk_update_for_table_tx(&mut tx, shape, filter, update, perm_filter).await?;
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    Ok(outcome)
}

/// Update one row by primary key inside a caller-owned transaction. Returns
/// `(old_row, new_row)`. `updated_at` is auto-refreshed when present — except
/// for empty bodies, which return the unchanged old row without refreshing it.
pub async fn execute_update_one_for_table_tx(
    tx: &mut Transaction<'_, Postgres>,
    shape: &TableShape,
    pk_value: &Value,
    body: &Map<String, Value>,
) -> Result<(Value, Value), AppError> {
    let pk = shape.single_pk()?;
    let table = qualified_table(&shape.schema, &shape.name);

    let keys: Vec<String> = body.keys().cloned().collect();
    validate_fields_for_write_shape(&keys, shape)?;

    let old_sql = format!(
        "SELECT row_to_json({}.*) FROM {} WHERE {} = $1{}",
        table,
        table,
        filter_compiler::quote(&pk.name),
        column_cast(pk),
    );
    let mut old_q = sqlx::query_as::<_, (Value,)>(&old_sql);
    old_q = crate::bind_json_value_owned!(old_q, pk_value.clone());
    let (old_item,): (Value,) = old_q
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to fetch old row: {}", e),
        })?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Row not found in table '{}.{}'",
                shape.schema, shape.name
            ))
        })?;

    if body.is_empty() {
        return Ok((old_item.clone(), old_item));
    }

    let mut bind_values: Vec<Value> = Vec::new();
    let mut set_clauses: Vec<String> = Vec::new();
    for (key, value) in body {
        let column = shape
            .columns
            .iter()
            .find(|c| c.name == *key)
            .ok_or_else(|| AppError::BadRequest(format!("Unknown field: '{}'", key)))?;
        let idx = bind_values.len() + 1;
        set_clauses.push(format!(
            "{} = ${}{}",
            filter_compiler::quote(key),
            idx,
            column_cast(column)
        ));
        bind_values.push(coerce_physical_value(value.clone(), column));
    }
    auto_updated_at(shape, body, &mut set_clauses);

    let pk_idx = bind_values.len() + 1;
    let sql = format!(
        "UPDATE {} SET {} WHERE {} = ${}{} RETURNING row_to_json({}.*)",
        table,
        set_clauses.join(", "),
        filter_compiler::quote(&pk.name),
        pk_idx,
        column_cast(pk),
        table,
    );

    let mut q = sqlx::query_as::<_, (Value,)>(&sql);
    for val in &bind_values {
        q = crate::bind_json_value!(q, val);
    }
    q = crate::bind_json_value_owned!(q, pk_value.clone());
    let (updated,): (Value,) =
        q.fetch_optional(&mut **tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Table update failed: {}", e),
            })?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Row not found in table '{}.{}'",
                    shape.schema, shape.name
                ))
            })?;

    Ok((old_item, updated))
}

/// Update one row by primary key on a physical (non-collection) table.
///
/// Scalar columns only (no relational processing). The old row is fetched for
/// `pairs`/delete-event parity. Single-column primary keys are required.
pub async fn execute_update_one_for_table(
    pool: &Pool,
    shape: &TableShape,
    pk_value: &Value,
    body: &Map<String, Value>,
) -> Result<WriteOutcome, AppError> {
    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    let (old_item, updated) = execute_update_one_for_table_tx(&mut tx, shape, pk_value, body).await?;
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    Ok(WriteOutcome {
        affected: vec![updated.clone()],
        pairs: vec![(old_item, updated)],
        ..Default::default()
    })
}

/// Delete rows by primary key inside a caller-owned transaction. Returns the
/// deleted rows.
pub async fn execute_delete_for_table_tx(
    tx: &mut Transaction<'_, Postgres>,
    shape: &TableShape,
    pks: &[Value],
) -> Result<Vec<Value>, AppError> {
    if pks.is_empty() {
        return Ok(Vec::new());
    }
    let pk = shape.single_pk()?;
    let table = qualified_table(&shape.schema, &shape.name);
    let placeholders: Vec<String> = (1..=pks.len())
        .map(|idx| format!("${}{}", idx, column_cast(pk)))
        .collect();
    let sql = format!(
        "DELETE FROM {} WHERE {} IN ({}) RETURNING row_to_json({}.*)",
        table,
        filter_compiler::quote(&pk.name),
        placeholders.join(", "),
        table,
    );
    let mut q = sqlx::query_as::<_, (Value,)>(&sql);
    for value in pks {
        q = crate::bind_json_value_owned!(q, value.clone());
    }
    let rows: Vec<(Value,)> = q.fetch_all(&mut **tx).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Table delete failed: {}", e),
        }
    })?;
    Ok(rows.into_iter().map(|(row,)| row).collect())
}

/// Delete rows by primary key on a physical (non-collection) table.
///
/// Deleted rows are returned under both `deleted` (old values for delete
/// events) and `affected`. Single-column primary keys are required.
pub async fn execute_delete_for_table(
    pool: &Pool,
    shape: &TableShape,
    pks: Vec<Value>,
) -> Result<WriteOutcome, AppError> {
    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    let rows = execute_delete_for_table_tx(&mut tx, shape, &pks).await?;
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    Ok(WriteOutcome {
        affected_count: rows.len() as u64,
        affected: rows.clone(),
        deleted: rows,
        ..Default::default()
    })
}

/// Delete rows matching `filter` on a physical table. No PK requirement, so
/// composite-PK tables (e.g. `alcedocore_menu_roles`) work.
pub async fn execute_delete_for_table_by_filter_tx(
    tx: &mut Transaction<'_, Postgres>,
    shape: &TableShape,
    filter: FilterCondition,
) -> Result<Vec<Value>, AppError> {
    let table = qualified_table(&shape.schema, &shape.name);
    let col_type_map = shape.col_type_map_ref();
    let mut bind_values: Vec<Value> = Vec::new();
    let clause = filter_compiler::compile_filter(&filter, &col_type_map, &mut bind_values)?;
    let sql = format!(
        "DELETE FROM {} WHERE {} RETURNING row_to_json({}.*)",
        table, clause, table,
    );
    let mut q = sqlx::query_as::<_, (Value,)>(&sql);
    for val in &bind_values {
        q = crate::bind_json_value!(q, val);
    }
    let rows: Vec<(Value,)> = q.fetch_all(&mut **tx).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Table delete failed: {}", e),
        }
    })?;
    Ok(rows.into_iter().map(|(row,)| row).collect())
}

/// Delete rows matching `filter` on a physical (non-collection) table.
///
/// No primary key is required, so tables with composite primary keys (e.g.
/// `alcedocore_menu_roles`) can be addressed. Deleted rows are returned under
/// both `deleted` and `affected`.
pub async fn execute_delete_for_table_by_filter(
    pool: &Pool,
    shape: &TableShape,
    filter: FilterCondition,
) -> Result<WriteOutcome, AppError> {
    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    let rows = execute_delete_for_table_by_filter_tx(&mut tx, shape, filter).await?;
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    Ok(WriteOutcome {
        affected_count: rows.len() as u64,
        affected: rows.clone(),
        deleted: rows,
        ..Default::default()
    })
}
