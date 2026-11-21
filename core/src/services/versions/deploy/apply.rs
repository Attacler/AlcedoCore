//! Applying edits to production: row-by-row application of selected edits
//! (`apply_selected_edits`) and the per-app write orchestration (`write_app`:
//! selected vs wholesale overwrite vs new-app clone).

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use sqlx::{Acquire, PgPool};

use super::{
    AppLink, DeployEdit, is_applicable, pk_columns, qident, qtable, table_columns, table_exists,
};
use crate::{
    migrations::app_migrations::m00011_seed_app_roles::seed_roles,
    services::{
        apps::AppsService,
        collections::schema::SchemaService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        versions::{
            clone_production::{
                clone_schema_batched, exec_multi, fix_table_sequences, missing_user_columns_sql,
                overwrite_schema_batched, recreate_schema_foreign_keys,
                rewrite_collection_app_columns, sync_missing_user_fks,
            },
            service::{PRODUCTION_VERSION, VersionsService},
        },
    },
};

/// The row key of an edit: the pk value from the system-log `metadata` (under
/// the pk column name, falling back to `id`), else the edit's `item_id`.
/// Handles both string and numeric pk values.
pub(crate) fn edit_key(meta: Option<&Value>, item_id: Option<&Value>, pk: &str) -> Option<String> {
    let v = meta
        .and_then(|m| m.get(pk).or_else(|| m.get("id")))
        .or(item_id)?;
    v.as_str()
        .map(String::from)
        .or_else(|| v.as_i64().map(|n| n.to_string()))
}

/// Applies the selected system-log edits row-by-row: for each edit on a
/// copy-set table, the current row of that target+pk in the version schema is
/// upserted into production (`*_created`/`*_updated`) or deleted there
/// (`*_deleted`). Runs in ONE transaction per app, so a failure changes
/// nothing. Catalog reads (pk/columns/existence) go through the transaction
/// itself, so they see tables created earlier in the same transaction.
pub(crate) async fn apply_selected_edits(
    pool: &PgPool,
    source_schema: &str,
    target_schema: &str,
    edits: &[&DeployEdit],
    copy_tables: &[String],
) -> Result<Vec<Value>, Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin().await?;
    let mut applied: Vec<Value> = Vec::new();
    // Per-target catalog cache: (single pk column, all columns).
    let mut catalog: HashMap<String, (Option<String>, Vec<String>)> = HashMap::new();
    for edit in edits {
        let Some(target) = edit.target.clone() else { continue };
        if !copy_tables.iter().any(|t| t == &target) {
            continue;
        }
        let (pk, cols) = match catalog.get(&target) {
            Some(cached) => cached.clone(),
            None => {
                let pk_cols = pk_columns(&mut *tx, target_schema, &target).await?;
                let pk = (pk_cols.len() == 1).then(|| pk_cols[0].clone());
                let cols = table_columns(&mut *tx, target_schema, &target).await?;
                catalog.insert(target.clone(), (pk.clone(), cols.clone()));
                (pk, cols)
            }
        };
        let Some(pk) = pk else {
            continue; // composite pk: not supported for row-level edits
        };
        let Some(key) = edit_key(edit.meta.as_ref(), edit.item_id.as_ref(), &pk) else {
            continue;
        };

        if edit.action.ends_with("_deleted") {
            sqlx::query(&format!(
                "DELETE FROM {} WHERE {}::text = $1",
                qtable(target_schema, &target),
                qident(&pk),
            ))
            .bind(&key)
            .execute(&mut *tx)
            .await?;
        } else {
            // A collection created in the version needs its user table in
            // production (structure-only) before its metadata row is applied.
            if edit.action == "collection_created" {
                if let Some(name) = &edit.meta_name {
                    if !table_exists(&mut *tx, target_schema, name).await? {
                        sqlx::query(&format!(
                            "CREATE TABLE {} (LIKE {} INCLUDING ALL)",
                            qtable(target_schema, name),
                            qtable(source_schema, name),
                        ))
                        .execute(&mut *tx)
                        .await?;
                        fix_table_sequences(&mut tx, source_schema, target_schema, name).await?;
                    }
                }
            }
            let set_clause = cols
                .iter()
                .filter(|c| c.as_str() != pk.as_str())
                .map(|c| format!("{} = EXCLUDED.{}", qident(c), qident(c)))
                .collect::<Vec<_>>()
                .join(", ");
            sqlx::query(&format!(
                "INSERT INTO {target} OVERRIDING SYSTEM VALUE \
                 SELECT * FROM {source} WHERE {pk}::text = $1 \
                 ON CONFLICT ({pk}) DO UPDATE SET {set_clause}",
                target = qtable(target_schema, &target),
                source = qtable(source_schema, &target),
                pk = qident(&pk),
                set_clause = set_clause,
            ))
            .bind(&key)
            .execute(&mut *tx)
            .await?;
            fix_table_sequences(&mut tx, source_schema, target_schema, &target).await?;
        }
        applied.push(json!({
            "id": edit.id,
            "app": edit.app,
            "action": edit.action,
            "target": target,
            "item_id": edit.item_id,
        }));
    }
    // Field edits only upsert the `alcedocore_fields` metadata row — add the
    // physical column to the target's user table as well, or the items API
    // rejects writes with "Unknown field". Only the collections that have
    // field edits in this batch are synced.
    let mut field_tables: Vec<String> = Vec::new();
    for edit in edits {
        if edit.target.as_deref() != Some("alcedocore_fields") {
            continue;
        }
        let Some(cid) = edit
            .meta
            .as_ref()
            .and_then(|m| m.get("collection_id"))
            .and_then(Value::as_i64)
        else {
            continue;
        };
        let table: Option<(String,)> = sqlx::query_as(&format!(
            "SELECT {} FROM {} WHERE id = $1",
            qident("table"),
            qtable(source_schema, "alcedocore_collections"),
        ))
        .bind(cid)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((t,)) = table {
            if !field_tables.contains(&t) {
                field_tables.push(t);
            }
        }
    }
    if !field_tables.is_empty() {
        let sync =
            missing_user_columns_sql(&mut *tx, source_schema, target_schema, Some(&field_tables))
                .await?;
        if !sync.is_empty() {
            exec_multi(&mut *tx, &sync).await?;
        }
    }
    // Relation fields added in the version create a physical FK in the version
    // schema; sync the FKs missing on the target's user tables as well — for
    // the collections with field edits (same list as the column sync) plus
    // collections created by selected edits (their table was just created
    // structure-only via LIKE, which never copies FKs). NOT VALID fallback
    // keeps orphaned production rows from failing the deploy.
    let mut fk_tables = field_tables.clone();
    for edit in edits {
        if edit.action == "collection_created" {
            if let Some(name) = &edit.meta_name {
                if !fk_tables.contains(name) {
                    fk_tables.push(name.clone());
                }
            }
        }
    }
    if !fk_tables.is_empty() {
        sync_missing_user_fks(&mut *tx, source_schema, target_schema, Some(&fk_tables)).await?;
    }
    // Rows copied from the version carry its app_version; re-point them at
    // the target schema so the meta cache resolves them.
    rewrite_collection_app_columns(&mut tx, target_schema).await?;
    tx.commit().await?;
    Ok(applied)
}

/// What one app's write produced: the per-app result entry for the response
/// plus the ids recorded in the `version_deployed` audit marker.
pub(crate) struct AppWriteResult {
    pub app: String,
    pub result: Value,
    pub system_ids: Vec<i64>,
    pub collection_ids: Vec<i64>,
    pub apps: Vec<String>,
}

/// Writes one linked app to production: selected edits row-by-row, wholesale
/// overwrite of the copy set, or the new-app flow (link + clone + seed roles
/// + recreate FKs) when the app is not linked to production yet.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn write_app(
    svc: &VersionsService<'_>,
    link: &AppLink,
    version_slug: &str,
    production_id: i32,
    prod_by_app: &HashMap<i32, AppLink>,
    selected_mode: bool,
    edits: &[DeployEdit],
    skipped: &HashSet<usize>,
    copy_tables_by_app: &HashMap<String, Vec<String>>,
) -> Result<AppWriteResult, AlcedoError> {
    let pool = &*svc.app_state.database_pool;
    let ver_schema = format!("{}010{}", link.api_name, version_slug);
    let copy_tables = copy_tables_by_app
        .get(&link.api_name)
        .cloned()
        .unwrap_or_default();
    match prod_by_app.get(&link.app_id) {
        Some(_) => {
            let prod_schema = format!("{}010production", link.api_name);
            if selected_mode {
                let applicable: Vec<&DeployEdit> = edits
                    .iter()
                    .enumerate()
                    .filter(|(i, e)| {
                        e.app == link.api_name
                            && !skipped.contains(i)
                            && is_applicable(e, copy_tables_by_app.get(&link.api_name))
                    })
                    .map(|(_, e)| e)
                    .collect();
                let applied = apply_selected_edits(
                    pool,
                    &ver_schema,
                    &prod_schema,
                    &applicable,
                    &copy_tables,
                )
                .await
                .map_err(|e| {
                    AlcedoError::SystemError(
                        format!(
                            "Failed to apply selected edits of {} to {}: {}",
                            ver_schema, prod_schema, e
                        ),
                        0,
                    )
                })?;
                let system_ids = applied
                    .iter()
                    .filter_map(|v| v.get("id").and_then(Value::as_i64))
                    .collect();
                Ok(AppWriteResult {
                    app: link.api_name.clone(),
                    result: json!({
                        "app": link.api_name,
                        "status": "selected",
                        "applied": applied,
                    }),
                    system_ids,
                    collection_ids: Vec::new(),
                    apps: Vec::new(),
                })
            } else {
                overwrite_schema_batched(
                    pool.clone(),
                    ver_schema.clone(),
                    prod_schema.clone(),
                    copy_tables,
                )
                .await
                .map_err(|e| {
                    AlcedoError::SystemError(
                        format!(
                            "Failed to deploy schema {} to {}: {}",
                            ver_schema, prod_schema, e
                        ),
                        0,
                    )
                })?;
                let system_ids = edits
                    .iter()
                    .enumerate()
                    .filter(|(i, e)| {
                        e.app == link.api_name && e.source == "system" && !skipped.contains(i)
                    })
                    .filter_map(|(_, e)| e.id)
                    .collect();
                let collection_ids = edits
                    .iter()
                    .enumerate()
                    .filter(|(i, e)| {
                        e.app == link.api_name && e.source == "collection" && !skipped.contains(i)
                    })
                    .filter_map(|(_, e)| e.id)
                    .collect();
                Ok(AppWriteResult {
                    app: link.api_name.clone(),
                    result: json!({ "app": link.api_name, "status": "overwritten" }),
                    system_ids,
                    collection_ids,
                    apps: Vec::new(),
                })
            }
        }
        None => {
            // New app: link it to production and clone its schema from the version.
            AppsService::new(svc.app_state)
                .link_apps_to_version(&[(link.app_id, production_id)])
                .await?;
            let prod_schema = format!("{}010production", link.api_name);
            clone_schema_batched(pool.clone(), ver_schema.clone(), prod_schema.clone(), copy_tables)
                .await
                .map_err(|e| {
                    AlcedoError::SystemError(
                        format!(
                            "Failed to clone schema {} to {}: {}",
                            ver_schema, prod_schema, e
                        ),
                        0,
                    )
                })?;
            // The migrator table is copied, so m00011 never runs for the new
            // production schema — seed its roles BEFORE the FKs are recreated
            // (same order as clone_production_apps).
            SchemaService::refresh_schemas_scoped(svc.app_state, &[prod_schema.clone()]).await?;
            let app_context = AppContext {
                app_name: link.api_name.clone(),
                version: PRODUCTION_VERSION.to_string(),
                request_source: RequestSource::Migration,
                identity: None,
                request_id: None,
            };
            seed_roles(svc.app_state, &app_context).await?;
            recreate_schema_foreign_keys(pool, &ver_schema, &prod_schema)
                .await
                .map_err(|e| {
                    AlcedoError::SystemError(
                        format!("Failed to recreate foreign keys of {}: {}", prod_schema, e),
                        0,
                    )
                })?;
            Ok(AppWriteResult {
                app: link.api_name.clone(),
                result: json!({ "app": link.api_name, "status": "created" }),
                system_ids: Vec::new(),
                collection_ids: Vec::new(),
                apps: vec![link.api_name.clone()],
            })
        }
    }
}
