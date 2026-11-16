use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{Postgres, Row, Transaction};
use utoipa::ToSchema;

pub(crate) mod ddl;
pub(crate) mod diff;
pub(crate) mod schema;
pub(crate) mod validate;

use ddl::*;
use diff::*;
use schema::SchemaService;
use validate::*;

use crate::{
    AppState, item_map,
    services::{
        context::AppContext,
        errors::AlcedoError,
        items::{
            query::Query,
            service::{ItemsService, TxGuard},
        },
        permissions::{
            self,
            create::{create_permission_summary, resolve_create_access},
        },
        postgres::pool::execute_query_transaction,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct FieldDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(rename = "type", default)]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub default_value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_collection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_parent_fields: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal_position: Option<i32>,
    #[serde(default)]
    pub is_system: bool,
}

impl FieldDefinition {
    pub(crate) fn is_virtual(&self) -> bool {
        self.field_type == "relationship"
            && self.relationship_type.as_deref() == Some("one_to_many")
    }

    pub(crate) fn is_relationship(&self) -> bool {
        self.field_type == "relationship"
    }

    pub(crate) fn is_user_field(&self) -> bool {
        self.field_type == "user"
    }

    pub(crate) fn is_fk_field(&self) -> bool {
        (self.is_relationship() && !self.is_virtual()) || self.is_user_field()
    }
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub struct CreateCollectionRequest {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub fields: Vec<FieldDefinition>,
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub struct UpdateCollectionRequest {
    #[serde(default)]
    pub fields: Vec<FieldDefinition>,
    #[serde(default)]
    pub removed_fields: Vec<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CollectionResponse {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub fields: Vec<FieldDefinition>,
    pub is_system: bool,
    pub created_at: Option<Value>,
    pub updated_at: Option<Value>,
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub struct SectionRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub section_type: Option<String>,
    #[serde(default)]
    pub relation_field: Option<String>,
    /// App that owns the related (child) collection. Absent = same app.
    #[serde(default)]
    pub related_app: Option<String>,
    #[serde(default)]
    pub view_type: Option<String>,
    #[serde(default)]
    pub default_filter: Option<Value>,
    #[serde(default)]
    pub display_fields: Option<Value>,
    #[serde(default)]
    pub item_limit: Option<i64>,
    #[serde(default)]
    pub ordinal_position: Option<i64>,
}

fn jstr(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn ji64(v: Option<&Value>) -> Option<i64> {
    v.and_then(|v| v.as_i64())
}

fn jbool(v: Option<&Value>) -> bool {
    v.and_then(|v| v.as_bool()).unwrap_or(false)
}

/// All collections in this context.
async fn read_collection_rows(
    state: &AppState,
    ctx: &AppContext,
) -> Result<Vec<Map<String, Value>>, AlcedoError> {
    let table = "alcedocore_collections".to_string();
    ItemsService::new(state, ctx, &table)
        .read_items_by_query(Query::default())
        .await
}

/// A single collection by its API name (the `table` column).
async fn read_collection_row(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<Map<String, Value>, AlcedoError> {
    let table = "alcedocore_collections".to_string();
    let mut rows = ItemsService::new(state, ctx, &table)
        .read_items_by_query(Query::eq("table", json!(name)))
        .await?;
    rows.pop()
        .ok_or_else(|| AlcedoError::NotFound(format!("Collection '{}' not found", name), 1))
}

/// Rows of a metadata table matching `filter`, ordered by `sort`.
async fn read_sorted(
    state: &AppState,
    ctx: &AppContext,
    table: &str,
    filter: Query,
    sort: &str,
) -> Result<Vec<Map<String, Value>>, AlcedoError> {
    let table = table.to_string();
    ItemsService::new(state, ctx, &table)
        .read_items_by_query(Query {
            filter: filter.filter,
            sort: vec![sort.to_string()],
            ..Default::default()
        })
        .await
}

async fn read_field_rows(
    state: &AppState,
    ctx: &AppContext,
    collection_id: i64,
) -> Result<Vec<Map<String, Value>>, AlcedoError> {
    read_sorted(
        state,
        ctx,
        "alcedocore_fields",
        Query::eq("collection_id", json!(collection_id)),
        "ordinal_position",
    )
    .await
}

fn field_from_row(row: &Map<String, Value>) -> FieldDefinition {
    let name = jstr(row.get("api_name")).unwrap_or_default();
    let display_name = jstr(row.get("display_name"));
    let ordinal = ji64(row.get("ordinal_position")).map(|v| v as i32);
    if let Some(Value::Object(opts)) = row.get("options") {
        if let Ok(mut field) =
            serde_json::from_value::<FieldDefinition>(Value::Object(opts.clone()))
        {
            field.name = name;
            field.display_name = display_name;
            field.ordinal_position = ordinal;
            return field;
        }
    }
    FieldDefinition {
        name,
        display_name,
        field_type: "string".to_string(),
        ordinal_position: ordinal,
        ..Default::default()
    }
}

fn field_payload(collection_id: i64, field: &FieldDefinition, index: usize) -> Map<String, Value> {
    let mut stored = field.clone();
    if stored.ordinal_position.is_none() {
        stored.ordinal_position = Some((index as i32) + 1);
    }
    let ordinal = stored.ordinal_position.unwrap_or((index as i32) + 1);
    item_map! {
        "collection_id" => collection_id,
        "api_name" => field.name,
        "display_name" => field.display_name.clone().unwrap_or_else(|| field.name.clone()),
        "ordinal_position" => ordinal,
        "options" => serde_json::to_value(&stored).unwrap_or(Value::Null),
    }
}

async fn build_collection_response(
    state: &AppState,
    ctx: &AppContext,
    row: &Map<String, Value>,
) -> Result<CollectionResponse, AlcedoError> {
    let collection_id = ji64(row.get("id")).unwrap_or(0);
    let fields = read_field_rows(state, ctx, collection_id)
        .await?
        .iter()
        .map(field_from_row)
        .collect();
    let namespaced = jstr(row.get("table")).unwrap_or_default();
    // Framework (`alcedocore_*`) collections are not user-managed: the admin
    // dashboard filters them out and the collection list hides Delete. This
    // includes `alcedocore_users`, which is policy-able but still a system
    // collection (it must not collide with the built-in Users page).
    let is_system = namespaced.starts_with("alcedocore");
    Ok(CollectionResponse {
        name: namespaced,
        display_name: jstr(row.get("name")),
        fields,
        is_system,
        created_at: row.get("created_at").cloned(),
        updated_at: row.get("updated_at").cloned(),
    })
}

async fn exec_all(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    sqls: Vec<String>,
) -> Result<(), AlcedoError> {
    for sql in sqls {
        execute_query_transaction(state, tx, &sql).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: collections
// ---------------------------------------------------------------------------

/// The physical table names of the collections in this context.
/// Lightweight alternative to `list_collections` for relationship detection.
pub async fn collection_tables(
    state: &AppState,
    ctx: &AppContext,
) -> Result<Vec<String>, AlcedoError> {
    let schema = ctx.schema_name();
    let guard = state.database_schema.read().await;
    Ok(guard
        .tables
        .iter()
        .filter(|t| t.schema == schema && t.meta.is_some())
        .map(|t| t.name.clone())
        .collect())
}

pub async fn list_collections(
    state: &AppState,
    ctx: &AppContext,
) -> Result<Vec<CollectionResponse>, AlcedoError> {
    let rows = read_collection_rows(state, ctx).await?;
    let accessible =
        permissions::read::list_accessible_collections(state, ctx, ctx.identity.as_ref()).await?;
    let rows = match &accessible {
        None => rows,
        Some(ids) => rows
            .into_iter()
            .filter(|row| {
                let table = jstr(row.get("table")).unwrap_or_default();
                crate::services::permissions::read::is_framework_collection(&table)
                    || ji64(row.get("id"))
                        .map(|id| ids.contains(&id))
                        .unwrap_or(false)
            })
            .collect(),
    };
    let mut result = Vec::new();
    for row in &rows {
        result.push(build_collection_response(state, ctx, row).await?);
    }
    result.sort_by(|a, b| {
        a.display_name
            .clone()
            .unwrap_or_else(|| a.name.clone())
            .cmp(&b.display_name.clone().unwrap_or_else(|| b.name.clone()))
    });
    Ok(result)
}

pub async fn get_collection(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<CollectionResponse, AlcedoError> {
    let row = read_collection_row(state, ctx, name).await?;
    build_collection_response(state, ctx, &row).await
}

/// Inserts the metadata rows for a collection — the `alcedocore_collections`
/// row, its `alcedocore_fields` rows, and a default layout + field-group
/// section — inside the caller's transaction, and queues the after-commit
/// hooks on the guard. Shared by `create_collection` (which runs the DDL
/// first) and `import_collections` (which adopts an existing table as-is).
async fn register_collection_meta(
    state: &AppState,
    ctx: &AppContext,
    guard: &mut TxGuard<'_>,
    name: &str,
    display_name: Option<String>,
    fields: &[FieldDefinition],
) -> Result<i64, AlcedoError> {
    let collections_table = "alcedocore_collections".to_string();
    let mut collections_service = ItemsService::new(state, ctx, &collections_table);
    let payload = item_map! {
        "app_name" => ctx.app_api_name(),
        "app_version" => ctx.version_api_name(),
        "table" => name,
        "name" => display_name.unwrap_or_else(|| name.to_string()),
        "singleton" => false,
        "hidden" => false,
    };
    let ids = collections_service
        .create_many(vec![payload], &mut Some(guard.tx()))
        .await?;
    let collection_id: i64 = ids
        .get(0)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AlcedoError::SystemError("Could not create collection".to_string(), 1))?;

    let fields_table = "alcedocore_fields".to_string();
    let mut fields_service = ItemsService::new(state, ctx, &fields_table);
    let field_payloads: Vec<Map<String, Value>> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| field_payload(collection_id, f, i))
        .collect();
    if !field_payloads.is_empty() {
        fields_service
            .create_many(field_payloads, &mut Some(guard.tx()))
            .await?;
    }

    // Seed a "Default" layout and a "Default" field-group section so a freshly
    // created collection opens directly in the builder with its fields shown.
    let layouts_table = "alcedocore_collection_layouts".to_string();
    let mut layouts_service = ItemsService::new(state, ctx, &layouts_table);
    let layout_payload = item_map! {
        "collection_id" => collection_id,
        "name" => "Default",
        "is_default" => true,
        "ordinal_position" => 1,
    };
    let layout_ids = layouts_service
        .create_many(vec![layout_payload], &mut Some(guard.tx()))
        .await?;
    let layout_id = layout_ids.get(0).map(|s| s.to_string()).unwrap_or_default();

    let sections_table = "alcedocore_collection_sections".to_string();
    let mut sections_service = ItemsService::new(state, ctx, &sections_table);
    let section_payload = item_map! {
        "collection_id" => collection_id,
        "layout_id" => layout_id,
        "name" => "Default",
        "section_type" => "field_group",
        "view_type" => "table",
        "item_limit" => 25,
        "ordinal_position" => 1,
        "display_fields" => Value::Array(
            fields
                .iter()
                .map(|f| Value::String(f.name.clone()))
                .collect(),
        ),
        "default_filter" => Value::Null,
    };
    sections_service
        .create_many(vec![section_payload], &mut Some(guard.tx()))
        .await?;

    guard.append(collections_service.take_pending_after());
    guard.append(fields_service.take_pending_after());
    guard.append(layouts_service.take_pending_after());
    guard.append(sections_service.take_pending_after());
    Ok(collection_id)
}

pub async fn create_collection(
    state: &AppState,
    ctx: &AppContext,
    req: CreateCollectionRequest,
) -> Result<CollectionResponse, AlcedoError> {
    validate_collection_name(&req.name)?;
    validate_fields(&req.fields)?;
    validate_related_apps(state, ctx, &req.fields).await?;

    let create_sql = build_create_table_sql(ctx, &req.name, &req.fields)?;

    let mut guard = TxGuard::new(state.database_pool.begin().await?);
    execute_query_transaction(state, guard.tx(), &create_sql).await?;
    register_collection_meta(
        state,
        ctx,
        &mut guard,
        &req.name,
        req.display_name.clone(),
        &req.fields,
    )
    .await?;

    let unique_sqls: Vec<String> = req
        .fields
        .iter()
        .filter(|f| f.unique && !f.is_virtual())
        .map(|f| build_add_unique_sql(ctx, &req.name, &f.name))
        .collect();
    exec_all(state, guard.tx(), unique_sqls).await?;

    // M:1 FKs for relationship fields; user fields FK to the global users table.
    let non_virtual_rel: Vec<&FieldDefinition> =
        req.fields.iter().filter(|f| f.is_fk_field()).collect();
    exec_all(
        state,
        guard.tx(),
        build_add_fk_sqls(ctx, &req.name, &non_virtual_rel)?,
    )
    .await?;

    // `one_to_many` fields are virtual: the link is the child's M:1 field, so
    // they need no column/FK of their own.

    // Commit, then flush the `after` hooks the transaction deferred.
    guard.commit(state).await?;
    SchemaService::new(state, ctx).refresh_schema().await;

    get_collection(state, ctx, &req.name).await
}

pub async fn update_collection(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    req: UpdateCollectionRequest,
) -> Result<CollectionResponse, AlcedoError> {
    validate_fields(&req.fields)?;

    let collection_id = collection_id_for(state, ctx, name).await?;

    let current_fields: Vec<FieldDefinition> = read_field_rows(state, ctx, collection_id)
        .await?
        .iter()
        .map(field_from_row)
        .collect();

    // Merge: keep fields omitted from the request unless explicitly removed.
    let mut desired = req.fields.clone();
    let requested: HashSet<String> = desired.iter().map(|f| f.name.clone()).collect();
    let removed_set: HashSet<String> = req.removed_fields.iter().cloned().collect();
    for field in &current_fields {
        if !requested.contains(&field.name) && !removed_set.contains(&field.name) {
            desired.push(field.clone());
        }
    }

    validate_related_apps(state, ctx, &desired).await?;

    let (renamed, added, removed_names) = compute_field_changes(&current_fields, &desired);
    let constraint_changes =
        compute_constraint_property_changes(&current_fields, &desired, &renamed);

    let removed_rel: Vec<&str> = removed_names
        .iter()
        .copied()
        .filter(|n| {
            current_fields
                .iter()
                .any(|f| f.name == **n && f.is_fk_field())
        })
        .collect();
    let renamed_rel_old: Vec<&str> = renamed
        .iter()
        .filter(|(old, _)| {
            current_fields
                .iter()
                .any(|f| f.name == *old && f.is_fk_field())
        })
        .map(|(old, _)| *old)
        .collect();

    // Only non-virtual fields own a column on this table.
    let renamed_columns: Vec<(&str, &str)> = renamed
        .iter()
        .filter(|(old, _)| {
            current_fields
                .iter()
                .any(|f| f.name == *old && !f.is_virtual())
        })
        .copied()
        .collect();

    let mut guard = TxGuard::new(state.database_pool.begin().await?);

    // 1. Drop M:1 FK constraints (removed + renamed relationship fields).
    let mut fk_drop: Vec<&str> = removed_rel.clone();
    fk_drop.extend(renamed_rel_old.iter().copied());
    exec_all(state, guard.tx(), build_drop_fk_sqls(ctx, name, &fk_drop)).await?;

    // 2. Rename columns.
    exec_all(
        state,
        guard.tx(),
        build_rename_columns_sqls(ctx, name, &renamed_columns),
    )
    .await?;

    // 3. Drop removed columns, add new columns.
    let removed_no_rel: Vec<&str> = removed_names
        .iter()
        .copied()
        .filter(|n| {
            !current_fields
                .iter()
                .any(|f| f.name == **n && f.is_virtual())
        })
        .collect();
    exec_all(
        state,
        guard.tx(),
        build_drop_columns_sqls(ctx, name, &removed_no_rel),
    )
    .await?;
    // Columns added directly in the database already exist physically — skip
    // the ADD COLUMN DDL for them and only register their metadata below.
    let physical_columns: HashSet<String> = introspect_columns(state, ctx, &[name.to_string()])
        .await?
        .into_iter()
        .map(|(_, f)| f.name)
        .collect();
    let added_new_columns: Vec<&FieldDefinition> = added
        .iter()
        .copied()
        .filter(|f| {
            // Skip ADD COLUMN only when the column exists physically and is
            // not being dropped in this same request (a type change drops +
            // re-adds the column, so those must be re-created).
            removed_names.contains(&f.name.as_str()) || !physical_columns.contains(&f.name)
        })
        .collect();
    exec_all(
        state,
        guard.tx(),
        build_add_columns_sqls(ctx, name, &added_new_columns)?,
    )
    .await?;

    // Unique indexes for newly added fields.
    let added_unique: Vec<String> = added
        .iter()
        .copied()
        .filter(|f| f.unique && !f.is_virtual())
        .map(|f| build_add_unique_sql(ctx, name, &f.name))
        .collect();
    exec_all(state, guard.tx(), added_unique).await?;

    // 4. Constraint sync (unique / required / default).
    for (old, new) in &constraint_changes {
        if new.is_virtual() {
            continue;
        }
        if old.unique != new.unique {
            if new.unique {
                exec_all(
                    state,
                    guard.tx(),
                    vec![build_add_unique_sql(ctx, name, &new.name)],
                )
                .await?;
            } else {
                exec_all(
                    state,
                    guard.tx(),
                    vec![build_drop_unique_sql(ctx, name, &new.name)],
                )
                .await?;
            }
        }
        if old.required != new.required {
            exec_all(
                state,
                guard.tx(),
                vec![build_not_null_sql(ctx, name, &new.name, new.required)],
            )
            .await?;
        }
        if old.default_value != new.default_value {
            let sql = match &new.default_value {
                Some(v) if !v.is_null() => build_set_default_sql(ctx, name, new, v)?,
                _ => build_drop_default_sql(ctx, name, &new.name),
            };
            exec_all(state, guard.tx(), vec![sql]).await?;
        }
    }

    // 5. Add M:1 FKs for newly added relationship fields. Fields whose column
    // already existed physically (see step 3) keep their existing constraint.
    let added_rel: Vec<&FieldDefinition> = added
        .iter()
        .copied()
        .filter(|f| {
            f.is_fk_field()
                && (removed_names.contains(&f.name.as_str())
                    || !physical_columns.contains(&f.name))
        })
        .collect();
    exec_all(state, guard.tx(), build_add_fk_sqls(ctx, name, &added_rel)?).await?;

    // 6. Update display_name.
    if let Some(display) = &req.display_name {
        let sql = format!(
            "UPDATE {} SET name = '{}', updated_at = NOW() WHERE id = {}",
            qtable(ctx, "alcedocore_collections"),
            escape_sql_string(display),
            collection_id
        );
        execute_query_transaction(state, guard.tx(), &sql).await?;
    }

    // 7. Replace field metadata rows.
    let field_rows = read_field_rows(state, ctx, collection_id).await?;
    let ids: Vec<Value> = field_rows
        .iter()
        .filter_map(|r| r.get("id").cloned())
        .collect();
    let fields_table = "alcedocore_fields".to_string();
    let mut fields_service = ItemsService::new(state, ctx, &fields_table);
    if !ids.is_empty() {
        fields_service
            .delete_items_by_pks(ids, Some(guard.tx()))
            .await?;
    }
    let payloads: Vec<Map<String, Value>> = desired
        .iter()
        .enumerate()
        .map(|(i, f)| field_payload(collection_id, f, i))
        .collect();
    if !payloads.is_empty() {
        fields_service
            .create_many(payloads, &mut Some(guard.tx()))
            .await?;
    }

    // Commit, then flush the `after` hooks the transaction deferred.
    guard.append(fields_service.take_pending_after());
    guard.commit(state).await?;
    SchemaService::new(state, ctx).refresh_schema().await;

    get_collection(state, ctx, name).await
}

pub async fn delete_collection(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<(), AlcedoError> {
    let collection_id = collection_id_for(state, ctx, name).await?;

    let mut guard = TxGuard::new(state.database_pool.begin().await?);
    let drop_sql = drop_table_sql(ctx, name, true);
    execute_query_transaction(state, guard.tx(), &drop_sql).await?;
    let cleanup = [
        format!(
            "DELETE FROM {} WHERE collection_id = {}",
            qtable(ctx, "alcedocore_fields"),
            collection_id
        ),
        format!(
            "DELETE FROM {} WHERE collection_id = {}",
            qtable(ctx, "alcedocore_collection_layouts"),
            collection_id
        ),
        format!(
            "DELETE FROM {} WHERE id = {}",
            qtable(ctx, "alcedocore_collections"),
            collection_id
        ),
    ];
    for sql in cleanup {
        execute_query_transaction(state, guard.tx(), &sql).await?;
    }
    guard.commit(state).await?;
    SchemaService::new(state, ctx).refresh_schema().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: import (adopt existing tables that have no metadata yet)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UnregisteredColumn {
    pub name: String,
    pub field_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_collection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_app: Option<String>,
    pub required: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UnregisteredTable {
    pub name: String,
    pub columns: Vec<UnregisteredColumn>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UnregisteredTablesResponse {
    pub tables: Vec<UnregisteredTable>,
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub struct ImportFieldOverride {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub input_component: Option<String>,
    #[serde(default)]
    pub display_component: Option<String>,
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub struct ImportCollectionRequest {
    pub table: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Per-field presentation overrides, matched by column name. Field types
    /// are always derived from the physical columns, never from the request.
    #[serde(default)]
    pub fields: Vec<ImportFieldOverride>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UnregisteredFieldsResponse {
    pub collection: String,
    pub fields: Vec<UnregisteredColumn>,
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub struct ImportFieldsRequest {
    /// Per-field presentation overrides, matched by column name. Field types
    /// are always derived from the physical columns, never from the request.
    #[serde(default)]
    pub fields: Vec<ImportFieldOverride>,
}

/// Framework tables (`alcedocore*`) and migration bookkeeping tables are
/// never importable.
fn is_importable_table(name: &str) -> bool {
    !name.starts_with("alcedocore") && name != "schema_migrations" && !name.ends_with("_migrations")
}

const SYSTEM_COLUMNS: [&str; 3] = ["id", "created_at", "updated_at"];

/// Maps a physical column to a field definition. FK columns become
/// `relationship` (many_to_one) fields — or `user` when they point at the
/// global users table; everything else is mapped by Postgres type.
fn field_from_column(
    ctx: &AppContext,
    name: &str,
    data_type: &str,
    udt_name: &str,
    is_nullable: &str,
    fk: Option<(String, String)>,
) -> FieldDefinition {
    let mut field = FieldDefinition {
        name: name.to_string(),
        required: is_nullable == "NO",
        ..Default::default()
    };
    match fk {
        Some((ref_schema, ref_table)) => {
            if ref_schema == "alcedocore" && ref_table == GLOBAL_USERS_COLLECTION {
                field.field_type = "user".to_string();
            } else {
                field.field_type = "relationship".to_string();
                field.relationship_type = Some("many_to_one".to_string());
                field.related_collection = Some(ref_table);
                if ref_schema != ctx.schema_name() {
                    if let Some((app, _)) = ref_schema.split_once("010") {
                        field.related_app = Some(app.to_string());
                    }
                }
            }
        }
        None => {
            field.field_type = match data_type {
                "character varying" | "character" => "string",
                "text" => "text",
                "smallint" | "integer" | "bigint" => "int",
                "real" | "double precision" | "numeric" => "float",
                "boolean" => "boolean",
                "date" | "timestamp without time zone" | "timestamp with time zone" => "datetime",
                "uuid" => "uuid",
                _ if udt_name == "_uuid" => "file",
                _ => "text",
            }
            .to_string();
        }
    }
    field
}

// pg_catalog joins (all oid-indexed) instead of information_schema views:
// the views re-check privileges per relation and take seconds on a database
// with many schemas, while pg_catalog answers in milliseconds.
const COLUMNS_WITH_FK_SQL: &str = "
SELECT cl.relname AS table_name,
       a.attname AS column_name,
       CASE t.typname
           WHEN 'int2' THEN 'smallint'
           WHEN 'int4' THEN 'integer'
           WHEN 'int8' THEN 'bigint'
           WHEN 'float4' THEN 'real'
           WHEN 'float8' THEN 'double precision'
           WHEN 'numeric' THEN 'numeric'
           WHEN 'bool' THEN 'boolean'
           WHEN 'varchar' THEN 'character varying'
           WHEN 'bpchar' THEN 'character'
           WHEN 'timestamp' THEN 'timestamp without time zone'
           WHEN 'timestamptz' THEN 'timestamp with time zone'
           ELSE t.typname
       END AS data_type,
       t.typname AS udt_name,
       CASE WHEN a.attnotnull THEN 'NO' ELSE 'YES' END AS is_nullable,
       rn.nspname AS referenced_schema,
       rc.relname AS referenced_table
FROM pg_catalog.pg_class cl
JOIN pg_catalog.pg_namespace n ON n.oid = cl.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = cl.oid AND a.attnum > 0 AND NOT a.attisdropped
JOIN pg_catalog.pg_type t ON t.oid = a.atttypid
LEFT JOIN pg_catalog.pg_constraint con
       ON con.conrelid = cl.oid AND con.contype = 'f' AND a.attnum = ANY(con.conkey)
LEFT JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid
LEFT JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace
WHERE n.nspname = $1 AND cl.relname = ANY($2) AND cl.relkind IN ('r', 'p')
ORDER BY cl.relname, a.attnum";

/// Introspects the given tables in the context's schema, returning
/// `(table, field)` pairs in column order, skipping system columns.
async fn introspect_columns(
    state: &AppState,
    ctx: &AppContext,
    tables: &[String],
) -> Result<Vec<(String, FieldDefinition)>, AlcedoError> {
    if tables.is_empty() {
        return Ok(vec![]);
    }
    let rows = sqlx::query(COLUMNS_WITH_FK_SQL)
        .bind(ctx.schema_name())
        .bind(tables)
        .fetch_all(&*state.database_pool)
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let table: String = row.try_get("table_name").ok()?;
            let name: String = row.try_get("column_name").ok()?;
            if SYSTEM_COLUMNS.contains(&name.as_str()) {
                return None;
            }
            let data_type: String = row.try_get("data_type").ok()?;
            let udt_name: String = row.try_get("udt_name").ok()?;
            let is_nullable: String = row.try_get("is_nullable").ok()?;
            let fk = match (
                row.try_get::<Option<String>, _>("referenced_schema"),
                row.try_get::<Option<String>, _>("referenced_table"),
            ) {
                (Ok(Some(s)), Ok(Some(t))) => Some((s, t)),
                _ => None,
            };
            Some((
                table,
                field_from_column(ctx, &name, &data_type, &udt_name, &is_nullable, fk),
            ))
        })
        .collect())
}

/// Tables in this app schema that have no collection metadata yet, with their
/// columns pre-mapped to field types.
pub async fn list_unregistered(
    state: &AppState,
    ctx: &AppContext,
) -> Result<UnregisteredTablesResponse, AlcedoError> {
    // Refresh first so tables created since the last cache refresh show up.
    SchemaService::new(state, ctx).refresh_schema().await;
    let schema_name = ctx.schema_name();
    let tables: Vec<String> = {
        let schema = state.database_schema.read().await;
        schema
            .tables
            .iter()
            .filter(|t| t.schema == schema_name && t.meta.is_none() && is_importable_table(&t.name))
            .map(|t| t.name.clone())
            .collect()
    };
    let columns = introspect_columns(state, ctx, &tables).await?;
    Ok(UnregisteredTablesResponse {
        tables: tables
            .into_iter()
            .map(|name| UnregisteredTable {
                columns: columns
                    .iter()
                    .filter(|(t, _)| *t == name)
                    .map(|(_, f)| UnregisteredColumn {
                        name: f.name.clone(),
                        field_type: f.field_type.clone(),
                        related_collection: f.related_collection.clone(),
                        related_app: f.related_app.clone(),
                        required: f.required,
                    })
                    .collect(),
                name,
            })
            .collect(),
    })
}

/// Adopts an existing physical table as a collection: registers the metadata
/// rows (collection, fields, default layout/section) without running any DDL.
pub async fn import_collections(
    state: &AppState,
    ctx: &AppContext,
    req: ImportCollectionRequest,
) -> Result<CollectionResponse, AlcedoError> {
    SchemaService::new(state, ctx).refresh_schema().await;
    let schema_name = ctx.schema_name();
    let (exists, has_meta) = {
        let schema = state.database_schema.read().await;
        let table = schema
            .tables
            .iter()
            .find(|t| t.schema == schema_name && t.name == req.table);
        (
            table.is_some(),
            table.map(|t| t.meta.is_some()).unwrap_or(false),
        )
    };
    if has_meta {
        return Err(AlcedoError::InvalidInput(
            format!("Collection '{}' already exists", req.table),
            1,
        ));
    }
    if !exists {
        return Err(AlcedoError::NotFound(
            format!("Table '{}' not found in this app schema", req.table),
            1,
        ));
    }
    if !is_importable_table(&req.table) {
        return Err(AlcedoError::InvalidInput(
            format!("Table '{}' cannot be imported", req.table),
            1,
        ));
    }
    validate_collection_name(&req.table)?;

    let columns = introspect_columns(state, ctx, &[req.table.clone()]).await?;
    let mut fields: Vec<FieldDefinition> = columns.into_iter().map(|(_, f)| f).collect();
    for ov in &req.fields {
        if let Some(f) = fields.iter_mut().find(|f| f.name == ov.name) {
            if let Some(d) = &ov.display_name {
                f.display_name = Some(d.clone());
            }
            if let Some(c) = &ov.input_component {
                f.input_component = Some(c.clone());
            }
            if let Some(c) = &ov.display_component {
                f.display_component = Some(c.clone());
            }
        }
    }
    validate_fields(&fields)?;

    let mut guard = TxGuard::new(state.database_pool.begin().await?);
    // The has_meta check above runs outside the transaction, so two racing
    // imports of the same table could both insert meta rows. Serialize them
    // with a transaction-scoped advisory lock and re-check inside the tx.
    execute_query_transaction(
        state,
        guard.tx(),
        &format!(
            "SELECT pg_advisory_xact_lock(hashtext({}))",
            quote_str(&format!("{}:{}", schema_name, req.table))
        ),
    )
    .await?;
    let dup = execute_query_transaction(
        state,
        guard.tx(),
        &format!(
            "SELECT 1 FROM {}.\"alcedocore_collections\" WHERE \"app_name\" = {} AND \"app_version\" = {} AND \"table\" = {} LIMIT 1",
            quote(&schema_name),
            quote_str(&ctx.app_api_name()),
            quote_str(&ctx.version_api_name()),
            quote_str(&req.table)
        ),
    )
    .await?;
    if !dup.is_empty() {
        return Err(AlcedoError::InvalidInput(
            format!("Collection '{}' already exists", req.table),
            1,
        ));
    }
    register_collection_meta(
        state,
        ctx,
        &mut guard,
        &req.table,
        req.display_name.clone(),
        &fields,
    )
    .await?;
    guard.commit(state).await?;
    SchemaService::new(state, ctx).refresh_schema().await;

    get_collection(state, ctx, &req.table).await
}

/// Columns of an existing collection that have no field metadata yet (e.g.
/// added directly in the database), with types mapped from the physical
/// columns.
pub async fn list_unregistered_fields(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<UnregisteredFieldsResponse, AlcedoError> {
    // Refresh first so columns added since the last cache refresh show up.
    SchemaService::new(state, ctx).refresh_schema().await;
    let collection_id = collection_id_for(state, ctx, name).await?;
    let registered: HashSet<String> = read_field_rows(state, ctx, collection_id)
        .await?
        .iter()
        .filter_map(|r| jstr(r.get("api_name")))
        .collect();
    let columns = introspect_columns(state, ctx, &[name.to_string()]).await?;
    Ok(UnregisteredFieldsResponse {
        collection: name.to_string(),
        fields: columns
            .into_iter()
            .filter(|(_, f)| !registered.contains(&f.name))
            .map(|(_, f)| UnregisteredColumn {
                name: f.name.clone(),
                field_type: f.field_type.clone(),
                related_collection: f.related_collection.clone(),
                related_app: f.related_app.clone(),
                required: f.required,
            })
            .collect(),
    })
}

/// Adopts existing physical columns of a collection as fields: registers the
/// field metadata rows without running any DDL (the columns already exist).
pub async fn import_fields(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    req: ImportFieldsRequest,
) -> Result<CollectionResponse, AlcedoError> {
    SchemaService::new(state, ctx).refresh_schema().await;
    let schema_name = ctx.schema_name();
    let collection_id = collection_id_for(state, ctx, name).await?;

    let physical = introspect_columns(state, ctx, &[name.to_string()]).await?;
    let existing_rows = read_field_rows(state, ctx, collection_id).await?;
    let registered: HashSet<String> = existing_rows
        .iter()
        .filter_map(|r| jstr(r.get("api_name")))
        .collect();

    let mut fields: Vec<FieldDefinition> = Vec::new();
    for ov in &req.fields {
        if registered.contains(&ov.name) {
            return Err(AlcedoError::InvalidInput(
                format!("Field '{}' already exists on collection '{}'", ov.name, name),
                1,
            ));
        }
        let mut field = physical
            .iter()
            .find(|(_, f)| f.name == ov.name)
            .map(|(_, f)| f.clone())
            .ok_or_else(|| {
                AlcedoError::NotFound(
                    format!("Column '{}' not found on table '{}'", ov.name, name),
                    1,
                )
            })?;
        if let Some(d) = &ov.display_name {
            field.display_name = Some(d.clone());
        }
        if let Some(c) = &ov.input_component {
            field.input_component = Some(c.clone());
        }
        if let Some(c) = &ov.display_component {
            field.display_component = Some(c.clone());
        }
        fields.push(field);
    }
    if fields.is_empty() {
        return Err(AlcedoError::InvalidInput("No fields to import".to_string(), 1));
    }
    validate_fields(&fields)?;

    let mut guard = TxGuard::new(state.database_pool.begin().await?);
    // The duplicate check above runs outside the transaction, so two racing
    // imports could both insert field rows. Serialize them with a
    // transaction-scoped advisory lock and re-check inside the tx.
    execute_query_transaction(
        state,
        guard.tx(),
        &format!(
            "SELECT pg_advisory_xact_lock(hashtext({}))",
            quote_str(&format!("{}:{}:fields", schema_name, name))
        ),
    )
    .await?;
    let names: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
    let in_list = names
        .iter()
        .map(|n| quote_str(n))
        .collect::<Vec<_>>()
        .join(",");
    let dup = execute_query_transaction(
        state,
        guard.tx(),
        &format!(
            "SELECT 1 FROM {}.\"alcedocore_fields\" WHERE collection_id = {} AND api_name IN ({}) LIMIT 1",
            quote(&schema_name),
            collection_id,
            in_list
        ),
    )
    .await?;
    if !dup.is_empty() {
        return Err(AlcedoError::InvalidInput(
            format!("A field on collection '{}' already exists", name),
            1,
        ));
    }
    // Compute the ordinal base inside the lock so concurrent imports of the
    // same collection cannot pick the same ordinals.
    let base_ordinal: i64 = execute_query_transaction(
        state,
        guard.tx(),
        &format!(
            "SELECT COUNT(*)::bigint FROM {}.\"alcedocore_fields\" WHERE collection_id = {}",
            quote(&schema_name),
            collection_id
        ),
    )
    .await?
    .first()
    .and_then(|r| r.try_get::<i64, _>(0).ok())
    .unwrap_or(0);

    let fields_table = "alcedocore_fields".to_string();
    let mut fields_service = ItemsService::new(state, ctx, &fields_table);
    let payloads: Vec<Map<String, Value>> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| field_payload(collection_id, f, base_ordinal as usize + i))
        .collect();
    fields_service
        .create_many(payloads, &mut Some(guard.tx()))
        .await?;
    guard.append(fields_service.take_pending_after());
    guard.commit(state).await?;
    SchemaService::new(state, ctx).refresh_schema().await;

    get_collection(state, ctx, name).await
}

pub async fn create_policy(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<Value, AlcedoError> {
    let collection = get_collection(state, ctx, name).await?;
    let access = resolve_create_access(state, ctx, name, ctx.identity.as_ref()).await?;
    Ok(create_permission_summary(&access, &collection.fields, name))
}

// ---------------------------------------------------------------------------
// Public API: layouts & sections
// ---------------------------------------------------------------------------

async fn collection_id_for(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<i64, AlcedoError> {
    state
        .database_schema
        .read()
        .await
        .collection_id(&ctx.schema_name(), name)
        .ok_or_else(|| AlcedoError::NotFound(format!("Collection '{}' not found", name), 1))
}

pub async fn list_layouts(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<Vec<Value>, AlcedoError> {
    let collection_id = collection_id_for(state, ctx, name).await?;
    let rows = read_sorted(
        state,
        ctx,
        "alcedocore_collection_layouts",
        Query::eq("collection_id", json!(collection_id)),
        "ordinal_position",
    )
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get("id").cloned().unwrap_or(Value::Null),
                "collection_name": name,
                "name": jstr(r.get("name")),
                "is_default": jbool(r.get("is_default")),
                "ordinal_position": ji64(r.get("ordinal_position")).unwrap_or(0),
                "created_at": r.get("created_at").cloned().unwrap_or(Value::Null),
                "updated_at": r.get("updated_at").cloned().unwrap_or(Value::Null),
            })
        })
        .collect())
}

pub async fn create_layout(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_name: &str,
) -> Result<Value, AlcedoError> {
    let collection_id = collection_id_for(state, ctx, name).await?;
    let rows = read_sorted(
        state,
        ctx,
        "alcedocore_collection_layouts",
        Query::eq("collection_id", json!(collection_id)),
        "ordinal_position",
    )
    .await?;
    let next = rows
        .iter()
        .filter_map(|r| ji64(r.get("ordinal_position")))
        .max()
        .unwrap_or(0)
        + 1;

    let table = "alcedocore_collection_layouts".to_string();
    let mut service = ItemsService::new(state, ctx, &table);
    let payload = item_map! {
        "collection_id" => collection_id,
        "name" => layout_name,
        "is_default" => false,
        "ordinal_position" => next,
    };
    let ids = service.create_many(vec![payload], &mut None).await?;
    let id = ids.get(0).map(|s| s.to_string()).unwrap_or_default();
    Ok(json!({ "id": id }))
}

pub async fn update_layout(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
    body: &Value,
) -> Result<Value, AlcedoError> {
    let collection_id = collection_id_for(state, ctx, name).await?;
    let table = "alcedocore_collection_layouts".to_string();
    let mut service = ItemsService::new(state, ctx, &table);

    let mut payload = Map::new();
    if let Some(v) = body.get("name").and_then(|v| v.as_str()) {
        if !v.trim().is_empty() {
            payload.insert("name".to_string(), json!(v));
        }
    }
    if let Some(v) = body.get("ordinal_position").and_then(|v| v.as_i64()) {
        payload.insert("ordinal_position".to_string(), json!(v));
    }
    let set_default = body.get("is_default").and_then(|v| v.as_bool());
    if set_default == Some(true) {
        let mut clear = Query::eq("collection_id", json!(collection_id));
        service
            .update_items_by_query(&mut clear, item_map! { "is_default" => false }, &mut None)
            .await?;
        payload.insert("is_default".to_string(), json!(true));
    }

    if !payload.is_empty() {
        let mut q = Query::eq("id", json!(layout_id));
        service
            .update_items_by_query(&mut q, payload, &mut None)
            .await?;
    }
    Ok(json!({ "updated": true }))
}

pub async fn delete_layout(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
) -> Result<Value, AlcedoError> {
    let collection_id = collection_id_for(state, ctx, name).await?;
    let rows = read_sorted(
        state,
        ctx,
        "alcedocore_collection_layouts",
        Query::eq("collection_id", json!(collection_id)),
        "ordinal_position",
    )
    .await?;
    if rows.len() <= 1 {
        return Err(AlcedoError::InvalidInput(
            "Cannot delete the last layout".to_string(),
            1,
        ));
    }
    let table = "alcedocore_collection_layouts".to_string();
    let mut service = ItemsService::new(state, ctx, &table);
    service
        .delete_items_by_pks(vec![json!(layout_id)], None)
        .await?;
    Ok(json!({ "deleted": true }))
}

async fn sections_for_layout(
    state: &AppState,
    ctx: &AppContext,
    layout_id: &str,
) -> Result<Vec<Map<String, Value>>, AlcedoError> {
    read_sorted(
        state,
        ctx,
        "alcedocore_collection_sections",
        Query::eq("layout_id", json!(layout_id)),
        "ordinal_position",
    )
    .await
}

fn section_json(row: &Map<String, Value>, collection_name: &str) -> Value {
    json!({
        "id": row.get("id").cloned().unwrap_or(Value::Null),
        "collection_name": collection_name,
        "name": jstr(row.get("name")),
        "section_type": jstr(row.get("section_type")).unwrap_or_else(|| "relational".to_string()),
        "relation_field": row.get("relation_field").cloned().unwrap_or(Value::Null),
        "related_app": row.get("related_app").cloned().unwrap_or(Value::Null),
        "view_type": row.get("view_type").cloned().unwrap_or(Value::Null),
        "default_filter": row.get("default_filter").cloned().unwrap_or(Value::Null),
        "display_fields": row.get("display_fields").cloned().unwrap_or(Value::Null),
        "item_limit": ji64(row.get("item_limit")).unwrap_or(25),
        "ordinal_position": ji64(row.get("ordinal_position")).unwrap_or(0),
        "created_at": row.get("created_at").cloned().unwrap_or(Value::Null),
        "updated_at": row.get("updated_at").cloned().unwrap_or(Value::Null),
    })
}

pub async fn list_sections(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
) -> Result<Vec<Value>, AlcedoError> {
    collection_id_for(state, ctx, name).await?;
    let mut rows = sections_for_layout(state, ctx, layout_id).await?;

    // Self-heal: ensure a layout always has at least a default "Fields" section.
    if rows.is_empty() {
        let collection_id = collection_id_for(state, ctx, name).await?;
        let fields: Vec<Value> = read_field_rows(state, ctx, collection_id)
            .await?
            .iter()
            .filter(|r| jstr(r.get("api_name")).is_some())
            .map(|r| json!(jstr(r.get("api_name")).unwrap_or_default()))
            .collect();
        let table = "alcedocore_collection_sections".to_string();
        let mut service = ItemsService::new(state, ctx, &table);
        let payload = item_map! {
            "collection_id" => collection_id,
            "layout_id" => layout_id,
            "name" => "Fields",
            "section_type" => "field_group",
            "view_type" => "table",
            "item_limit" => 25,
            "ordinal_position" => 1,
            "display_fields" => Value::Array(fields),
            "default_filter" => Value::Null,
        };
        service.create_many(vec![payload], &mut None).await?;
        rows = sections_for_layout(state, ctx, layout_id).await?;
    }

    Ok(rows.iter().map(|r| section_json(r, name)).collect())
}

pub async fn create_section(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
    req: SectionRequest,
) -> Result<Value, AlcedoError> {
    let collection_id = collection_id_for(state, ctx, name).await?;
    let existing = sections_for_layout(state, ctx, layout_id).await?;
    let next = existing
        .iter()
        .filter_map(|r| ji64(r.get("ordinal_position")))
        .max()
        .unwrap_or(0)
        + 1;

    let table = "alcedocore_collection_sections".to_string();
    let mut service = ItemsService::new(state, ctx, &table);
    let payload = item_map! {
        "collection_id" => collection_id,
        "layout_id" => layout_id,
        "name" => req.name,
        "section_type" => req.section_type.unwrap_or_else(|| "relational".to_string()),
        "relation_field" => req.relation_field.clone().map_or(Value::Null, Value::String),
        "related_app" => req.related_app.clone().map_or(Value::Null, Value::String),
        "view_type" => req.view_type.clone().map_or(Value::Null, Value::String),
        "default_filter" => req.default_filter.clone().unwrap_or(Value::Null),
        "display_fields" => req.display_fields.clone().unwrap_or(Value::Null),
        "item_limit" => req.item_limit.unwrap_or(25),
        "ordinal_position" => req.ordinal_position.unwrap_or(next),
    };
    let ids = service.create_many(vec![payload], &mut None).await?;
    let id = ids.get(0).map(|s| s.to_string()).unwrap_or_default();
    Ok(json!({ "id": id }))
}

pub async fn update_section(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
    section_id: &str,
    req: SectionRequest,
) -> Result<Value, AlcedoError> {
    collection_id_for(state, ctx, name).await?;
    let table = "alcedocore_collection_sections".to_string();
    let mut service = ItemsService::new(state, ctx, &table);

    let mut payload = item_map! {
        "name" => req.name,
        "relation_field" => req.relation_field.clone().map_or(Value::Null, Value::String),
        "related_app" => req.related_app.clone().map_or(Value::Null, Value::String),
        "view_type" => req.view_type.clone().map_or(Value::Null, Value::String),
        "default_filter" => req.default_filter.clone().unwrap_or(Value::Null),
        "display_fields" => req.display_fields.clone().unwrap_or(Value::Null),
        "item_limit" => req.item_limit.unwrap_or(25),
    };
    if let Some(st) = req.section_type {
        payload.insert("section_type".to_string(), json!(st));
    }
    if let Some(pos) = req.ordinal_position {
        payload.insert("ordinal_position".to_string(), json!(pos));
    }

    let mut q = Query::eq("id", json!(section_id));
    service
        .update_items_by_query(&mut q, payload, &mut None)
        .await?;
    let _ = layout_id;
    Ok(json!({ "updated": true }))
}

pub async fn delete_section(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
    section_id: &str,
) -> Result<Value, AlcedoError> {
    collection_id_for(state, ctx, name).await?;
    let table = "alcedocore_collection_sections".to_string();
    let mut service = ItemsService::new(state, ctx, &table);
    service
        .delete_items_by_pks(vec![json!(section_id)], None)
        .await?;
    let _ = layout_id;
    Ok(json!({ "deleted": true }))
}

pub async fn reorder_sections(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
    body: &Value,
) -> Result<Value, AlcedoError> {
    collection_id_for(state, ctx, name).await?;
    let table = "alcedocore_collection_sections".to_string();
    let mut service = ItemsService::new(state, ctx, &table);
    if let Some(sections) = body.get("sections").and_then(|v| v.as_array()) {
        for section in sections {
            let id = match section.get("id").and_then(|v| v.as_str()) {
                Some(id) => id,
                None => continue,
            };
            let pos = section
                .get("ordinal_position")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let payload = item_map! { "ordinal_position" => pos };
            let mut q = Query::eq("id", json!(id));
            service
                .update_items_by_query(&mut q, payload, &mut None)
                .await?;
        }
    }
    let _ = layout_id;
    Ok(json!({ "updated": true }))
}

pub async fn resolve_layout(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<Value, AlcedoError> {
    let layouts = list_layouts(state, ctx, name).await?;
    let layout = layouts
        .iter()
        .find(|l| {
            l.get("is_default")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .or_else(|| layouts.first())
        .cloned()
        .unwrap_or(Value::Null);
    let layout_id = layout
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let sections = match layout_id {
        Some(id) => list_sections(state, ctx, name, &id).await?,
        None => vec![],
    };
    let available: Vec<Value> = layouts
        .iter()
        .map(|l| json!({ "id": l.get("id"), "name": l.get("name") }))
        .collect();
    Ok(json!({
        "layout": { "id": layout.get("id").cloned().unwrap_or(Value::Null), "name": layout.get("name").cloned().unwrap_or(Value::Null) },
        "sections": sections,
        "available_layouts": available,
    }))
}

pub async fn get_layout_roles(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
) -> Result<Value, AlcedoError> {
    collection_id_for(state, ctx, name).await?;
    let sql = format!(
        "SELECT lr.role_id::text AS role_id, r.name AS role_name FROM {} lr JOIN {} r ON r.id = lr.role_id WHERE lr.layout_id = '{}'::uuid",
        qtable(ctx, "alcedocore_collection_layout_roles"),
        qtable(ctx, "alcedocore_roles"),
        escape_sql_string(layout_id)
    );
    let rows = crate::services::postgres::pool::execute_query(state, sql).await?;
    let roles: Vec<Value> = rows
        .iter()
        .filter_map(|row| crate::services::postgres::pool::pgrow_to_json(row).ok())
        .map(Value::Object)
        .collect();
    Ok(json!({ "roles": roles }))
}

pub async fn set_layout_roles(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
    layout_id: &str,
    body: &Value,
) -> Result<Value, AlcedoError> {
    collection_id_for(state, ctx, name).await?;
    let mut guard = TxGuard::new(state.database_pool.begin().await?);
    let delete_sql = format!(
        "DELETE FROM {} WHERE layout_id = '{}'::uuid",
        qtable(ctx, "alcedocore_collection_layout_roles"),
        escape_sql_string(layout_id)
    );
    execute_query_transaction(state, guard.tx(), &delete_sql).await?;

    if let Some(ids) = body.get("role_ids").and_then(|v| v.as_array()) {
        for role in ids {
            if let Some(role_id) = role.as_str() {
                if uuid::Uuid::parse_str(role_id).is_err() {
                    continue;
                }
                let insert = format!(
                    "INSERT INTO {} (layout_id, role_id) VALUES ('{}'::uuid, '{}'::uuid)",
                    qtable(ctx, "alcedocore_collection_layout_roles"),
                    escape_sql_string(layout_id),
                    escape_sql_string(role_id)
                );
                execute_query_transaction(state, guard.tx(), &insert).await?;
            }
        }
    }
    guard.commit(state).await?;
    Ok(json!({ "updated": true }))
}

pub async fn fields_from_schema(
    state: &AppState,
    ctx: &AppContext,
    table: &str,
) -> Option<Vec<FieldDefinition>> {
    let schema_name = ctx.schema_name();
    let schema = state.database_schema.read().await;
    schema
        .fields
        .iter()
        .find(|tf| tf.schema == schema_name && tf.table == table)
        .map(|tf| tf.fields.clone())
}

#[cfg(test)]
mod import_fields_tests {
    use super::*;
    use crate::services::context::RequestSource;
    use crate::services::postgres::pool::execute_query;
    use crate::utils;

    fn test_ctx() -> AppContext {
        AppContext {
            app_name: "testing".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::SystemTest,
            identity: None,
            request_id: None,
        }
    }

    async fn cleanup(state: &AppState, ctx: &AppContext, name: &str) {
        let _ = delete_collection(state, ctx, name).await;
        let _ = execute_query(
            state,
            format!(
                "DROP TABLE IF EXISTS {}.{}",
                quote(&ctx.schema_name()),
                quote(name)
            ),
        )
        .await;
    }

    #[tokio::test]
    async fn import_fields_adopts_existing_column_without_ddl() {
        let state = utils::test_utils::get_app_state().await;
        let ctx = test_ctx();
        let name = "automated_import_fields_test";
        cleanup(&state, &ctx, name).await;

        create_collection(
            &state,
            &ctx,
            CreateCollectionRequest {
                name: name.to_string(),
                display_name: None,
                fields: vec![FieldDefinition {
                    name: "title".to_string(),
                    field_type: "string".to_string(),
                    ..Default::default()
                }],
            },
        )
        .await
        .expect("create collection");

        // Simulate a column added directly in the database.
        execute_query(
            &state,
            format!(
                "ALTER TABLE {}.{} ADD COLUMN extra_col text, ADD COLUMN extra_num integer",
                quote(&ctx.schema_name()),
                quote(name)
            ),
        )
        .await
        .expect("add columns");

        let unreg = list_unregistered_fields(&state, &ctx, name)
            .await
            .expect("list unregistered fields");
        let names: Vec<&str> = unreg.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"extra_col") && names.contains(&"extra_num"));
        let col_type = unreg
            .fields
            .iter()
            .find(|f| f.name == "extra_col")
            .unwrap()
            .field_type
            .clone();
        assert_eq!(col_type, "text");

        let updated = import_fields(
            &state,
            &ctx,
            name,
            ImportFieldsRequest {
                fields: vec![
                    ImportFieldOverride {
                        name: "extra_col".to_string(),
                        display_name: Some("Extra col".to_string()),
                        input_component: None,
                        display_component: None,
                    },
                    ImportFieldOverride {
                        name: "extra_num".to_string(),
                        ..Default::default()
                    },
                ],
            },
        )
        .await
        .expect("import fields");
        let field_names: Vec<&str> = updated.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(field_names.contains(&"extra_col") && field_names.contains(&"extra_num"));

        // Columns were not re-created (still exactly one of each) and the
        // duplicate import is rejected.
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 AND column_name IN ('extra_col', 'extra_num')"
        ))
        .bind(ctx.schema_name())
        .bind(name)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(count, 2);

        let dup = import_fields(
            &state,
            &ctx,
            name,
            ImportFieldsRequest {
                fields: vec![ImportFieldOverride {
                    name: "extra_col".to_string(),
                    ..Default::default()
                }],
            },
        )
        .await;
        assert!(dup.is_err());

        let unreg = list_unregistered_fields(&state, &ctx, name).await.unwrap();
        assert!(unreg.fields.is_empty());

        cleanup(&state, &ctx, name).await;
    }

    #[tokio::test]
    async fn update_collection_skips_add_column_for_existing_column() {
        let state = utils::test_utils::get_app_state().await;
        let ctx = test_ctx();
        let name = "automated_update_existing_col_test";
        cleanup(&state, &ctx, name).await;

        create_collection(
            &state,
            &ctx,
            CreateCollectionRequest {
                name: name.to_string(),
                display_name: None,
                fields: vec![FieldDefinition {
                    name: "title".to_string(),
                    field_type: "string".to_string(),
                    ..Default::default()
                }],
            },
        )
        .await
        .expect("create collection");

        execute_query(
            &state,
            format!(
                "ALTER TABLE {}.{} ADD COLUMN manual_col text",
                quote(&ctx.schema_name()),
                quote(name)
            ),
        )
        .await
        .expect("add column");

        // Adding a field whose column already exists must not run ADD COLUMN.
        let updated = update_collection(
            &state,
            &ctx,
            name,
            UpdateCollectionRequest {
                fields: vec![
                    FieldDefinition {
                        name: "title".to_string(),
                        field_type: "string".to_string(),
                        ..Default::default()
                    },
                    FieldDefinition {
                        name: "manual_col".to_string(),
                        field_type: "text".to_string(),
                        display_name: Some("Manual col".to_string()),
                        ..Default::default()
                    },
                ],
                removed_fields: vec![],
                display_name: None,
            },
        )
        .await
        .expect("update collection with existing column must succeed");
        let field_names: Vec<&str> = updated.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(field_names.contains(&"manual_col"));

        // The column still exists exactly once.
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 AND column_name = 'manual_col'",
        )
        .bind(ctx.schema_name())
        .bind(name)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(count, 1);

        // A type change drops + re-adds the column — the re-add must run even
        // though the column existed physically before this request.
        update_collection(
            &state,
            &ctx,
            name,
            UpdateCollectionRequest {
                fields: vec![
                    FieldDefinition {
                        name: "title".to_string(),
                        field_type: "string".to_string(),
                        ..Default::default()
                    },
                    FieldDefinition {
                        name: "manual_col".to_string(),
                        field_type: "string".to_string(),
                        ..Default::default()
                    },
                ],
                removed_fields: vec![],
                display_name: None,
            },
        )
        .await
        .expect("type change must drop + re-add the column");
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 AND column_name = 'manual_col'",
        )
        .bind(ctx.schema_name())
        .bind(name)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(count, 1);

        cleanup(&state, &ctx, name).await;
    }
}
