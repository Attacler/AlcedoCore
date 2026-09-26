use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{Postgres, Transaction};
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
        items::{query::Query, service::ItemsService},
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
    let table = "alcedo_collections".to_string();
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
    let table = "alcedo_collections".to_string();
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
        "alcedo_fields",
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
    Ok(CollectionResponse {
        name: jstr(row.get("table")).unwrap_or_default(),
        display_name: jstr(row.get("name")),
        fields,
        is_system: false,
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

pub async fn create_collection(
    state: &AppState,
    ctx: &AppContext,
    req: CreateCollectionRequest,
) -> Result<CollectionResponse, AlcedoError> {
    validate_collection_name(&req.name)?;
    validate_fields(&req.fields)?;
    validate_related_apps(state, ctx, &req.fields).await?;

    let create_sql = build_create_table_sql(ctx, &req.name, &req.fields)?;

    let mut tx = state.database_pool.begin().await?;
    execute_query_transaction(state, &mut tx, &create_sql).await?;

    let collections_table = "alcedo_collections".to_string();
    let collections_service = ItemsService::new(state, ctx, &collections_table);
    let payload = item_map! {
        "app_name" => ctx.app_api_name(),
        "app_version" => ctx.version_api_name(),
        "table" => req.name,
        "name" => req.display_name.clone().unwrap_or_else(|| req.name.clone()),
        "singleton" => false,
        "hidden" => false,
    };
    let ids = collections_service
        .create_many(vec![payload], &mut Some(&mut tx))
        .await?;
    let collection_id: i64 = ids
        .get(0)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AlcedoError::SystemError("Could not create collection".to_string(), 1))?;

    let fields_table = "alcedo_fields".to_string();
    let fields_service = ItemsService::new(state, ctx, &fields_table);
    let field_payloads: Vec<Map<String, Value>> = req
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| field_payload(collection_id, f, i))
        .collect();
    if !field_payloads.is_empty() {
        fields_service
            .create_many(field_payloads, &mut Some(&mut tx))
            .await?;
    }

    // Seed a "Default" layout and a "Default" field-group section so a freshly
    // created collection opens directly in the builder with its fields shown.
    let layouts_table = "alcedo_collection_layouts".to_string();
    let layouts_service = ItemsService::new(state, ctx, &layouts_table);
    let layout_payload = item_map! {
        "collection_id" => collection_id,
        "name" => "Default",
        "is_default" => true,
        "ordinal_position" => 1,
    };
    let layout_ids = layouts_service
        .create_many(vec![layout_payload], &mut Some(&mut tx))
        .await?;
    let layout_id = layout_ids
        .get(0)
        .map(|s| s.to_string())
        .unwrap_or_default();

    let sections_table = "alcedo_collection_sections".to_string();
    let sections_service = ItemsService::new(state, ctx, &sections_table);
    let section_payload = item_map! {
        "collection_id" => collection_id,
        "layout_id" => layout_id,
        "name" => "Default",
        "section_type" => "field_group",
        "view_type" => "table",
        "item_limit" => 25,
        "ordinal_position" => 1,
        "display_fields" => Value::Array(
            req.fields
                .iter()
                .map(|f| Value::String(f.name.clone()))
                .collect(),
        ),
        "default_filter" => Value::Null,
    };
    sections_service
        .create_many(vec![section_payload], &mut Some(&mut tx))
        .await?;

    let unique_sqls: Vec<String> = req
        .fields
        .iter()
        .filter(|f| f.unique && !f.is_virtual())
        .map(|f| build_add_unique_sql(ctx, &req.name, &f.name))
        .collect();
    exec_all(state, &mut tx, unique_sqls).await?;

    let non_virtual_rel: Vec<&FieldDefinition> = req
        .fields
        .iter()
        .filter(|f| f.is_relationship() && !f.is_virtual())
        .collect();
    exec_all(
        state,
        &mut tx,
        build_add_fk_sqls(ctx, &req.name, &non_virtual_rel)?,
    )
    .await?;

    // `one_to_many` fields are virtual: the link is the child's M:1 field, so
    // they need no column/FK of their own.

    tx.commit().await?;
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
                .any(|f| f.name == **n && f.is_relationship())
        })
        .collect();
    let renamed_rel_old: Vec<&str> = renamed
        .iter()
        .filter(|(old, _)| {
            current_fields
                .iter()
                .any(|f| f.name == *old && f.is_relationship() && !f.is_virtual())
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

    let mut tx = state.database_pool.begin().await?;

    // 1. Drop M:1 FK constraints (removed + renamed relationship fields).
    let mut fk_drop: Vec<&str> = removed_rel.clone();
    fk_drop.extend(renamed_rel_old.iter().copied());
    exec_all(state, &mut tx, build_drop_fk_sqls(ctx, name, &fk_drop)).await?;

    // 2. Rename columns.
    exec_all(
        state,
        &mut tx,
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
        &mut tx,
        build_drop_columns_sqls(ctx, name, &removed_no_rel),
    )
    .await?;
    exec_all(state, &mut tx, build_add_columns_sqls(ctx, name, &added)?).await?;

    // Unique indexes for newly added fields.
    let added_unique: Vec<String> = added
        .iter()
        .copied()
        .filter(|f| f.unique && !f.is_virtual())
        .map(|f| build_add_unique_sql(ctx, name, &f.name))
        .collect();
    exec_all(state, &mut tx, added_unique).await?;

    // 4. Constraint sync (unique / required / default).
    for (old, new) in &constraint_changes {
        if new.is_virtual() {
            continue;
        }
        if old.unique != new.unique {
            if new.unique {
                exec_all(
                    state,
                    &mut tx,
                    vec![build_add_unique_sql(ctx, name, &new.name)],
                )
                .await?;
            } else {
                exec_all(
                    state,
                    &mut tx,
                    vec![build_drop_unique_sql(ctx, name, &new.name)],
                )
                .await?;
            }
        }
        if old.required != new.required {
            exec_all(
                state,
                &mut tx,
                vec![build_not_null_sql(ctx, name, &new.name, new.required)],
            )
            .await?;
        }
        if old.default_value != new.default_value {
            let sql = match &new.default_value {
                Some(v) if !v.is_null() => build_set_default_sql(ctx, name, new, v)?,
                _ => build_drop_default_sql(ctx, name, &new.name),
            };
            exec_all(state, &mut tx, vec![sql]).await?;
        }
    }

    // 5. Add M:1 FKs for newly added relationship fields.
    let added_rel: Vec<&FieldDefinition> = added
        .iter()
        .copied()
        .filter(|f| f.is_relationship() && !f.is_virtual())
        .collect();
    exec_all(state, &mut tx, build_add_fk_sqls(ctx, name, &added_rel)?).await?;

    // 6. Update display_name.
    if let Some(display) = &req.display_name {
        let sql = format!(
            "UPDATE {} SET name = '{}', updated_at = NOW() WHERE id = {}",
            qtable(ctx, "alcedo_collections"),
            escape_sql_string(display),
            collection_id
        );
        execute_query_transaction(state, &mut tx, &sql).await?;
    }

    // 7. Replace field metadata rows.
    let field_rows = read_field_rows(state, ctx, collection_id).await?;
    let ids: Vec<Value> = field_rows
        .iter()
        .filter_map(|r| r.get("id").cloned())
        .collect();
    let fields_table = "alcedo_fields".to_string();
    let fields_service = ItemsService::new(state, ctx, &fields_table);
    if !ids.is_empty() {
        fields_service
            .delete_items_by_pks(ids, Some(&mut tx))
            .await?;
    }
    let payloads: Vec<Map<String, Value>> = desired
        .iter()
        .enumerate()
        .map(|(i, f)| field_payload(collection_id, f, i))
        .collect();
    if !payloads.is_empty() {
        fields_service
            .create_many(payloads, &mut Some(&mut tx))
            .await?;
    }

    tx.commit().await?;
    SchemaService::new(state, ctx).refresh_schema().await;

    get_collection(state, ctx, name).await
}

pub async fn delete_collection(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<(), AlcedoError> {
    let collection_id = collection_id_for(state, ctx, name).await?;

    let mut tx = state.database_pool.begin().await?;
    let drop_sql = drop_table_sql(ctx, name, true);
    execute_query_transaction(state, &mut tx, &drop_sql).await?;
    let cleanup = [
        format!(
            "DELETE FROM {} WHERE collection_id = {}",
            qtable(ctx, "alcedo_fields"),
            collection_id
        ),
        format!(
            "DELETE FROM {} WHERE collection_id = {}",
            qtable(ctx, "alcedo_collection_layouts"),
            collection_id
        ),
        format!(
            "DELETE FROM {} WHERE id = {}",
            qtable(ctx, "alcedo_collections"),
            collection_id
        ),
    ];
    for sql in cleanup {
        execute_query_transaction(state, &mut tx, &sql).await?;
    }
    tx.commit().await?;
    SchemaService::new(state, ctx).refresh_schema().await;
    Ok(())
}

pub async fn create_policy(
    state: &AppState,
    ctx: &AppContext,
    name: &str,
) -> Result<Value, AlcedoError> {
    let collection = get_collection(state, ctx, name).await?;
    Ok(json!({
        "collection_name": name,
        "allowed_fields": collection.fields,
        "field_validation": [],
        "$permissions": { "create": true }
    }))
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
        "alcedo_collection_layouts",
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
        "alcedo_collection_layouts",
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

    let table = "alcedo_collection_layouts".to_string();
    let service = ItemsService::new(state, ctx, &table);
    let payload = item_map! {
        "collection_id" => collection_id,
        "name" => layout_name,
        "is_default" => false,
        "ordinal_position" => next,
    };
    let ids = service.create_many(vec![payload], &mut None).await?;
    let id = ids
        .get(0)
        .map(|s| s.to_string())
        .unwrap_or_default();
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
    let table = "alcedo_collection_layouts".to_string();
    let service = ItemsService::new(state, ctx, &table);

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
            .update_items_by_query(
                &mut clear,
                item_map! { "is_default" => false },
                &mut None,
            )
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
        "alcedo_collection_layouts",
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
    let table = "alcedo_collection_layouts".to_string();
    let service = ItemsService::new(state, ctx, &table);
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
        "alcedo_collection_sections",
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
        let table = "alcedo_collection_sections".to_string();
        let service = ItemsService::new(state, ctx, &table);
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

    let table = "alcedo_collection_sections".to_string();
    let service = ItemsService::new(state, ctx, &table);
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
    let id = ids
        .get(0)
        .map(|s| s.to_string())
        .unwrap_or_default();
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
    let table = "alcedo_collection_sections".to_string();
    let service = ItemsService::new(state, ctx, &table);

    let mut payload = Map::new();
    payload.insert("name".to_string(), json!(req.name));
    if let Some(st) = req.section_type {
        payload.insert("section_type".to_string(), json!(st));
    }
    payload.insert(
        "relation_field".to_string(),
        req.relation_field
            .clone()
            .map_or(Value::Null, Value::String),
    );
    payload.insert(
        "related_app".to_string(),
        req.related_app.clone().map_or(Value::Null, Value::String),
    );
    payload.insert(
        "view_type".to_string(),
        req.view_type.clone().map_or(Value::Null, Value::String),
    );
    payload.insert(
        "default_filter".to_string(),
        req.default_filter.clone().unwrap_or(Value::Null),
    );
    payload.insert(
        "display_fields".to_string(),
        req.display_fields.clone().unwrap_or(Value::Null),
    );
    payload.insert(
        "item_limit".to_string(),
        json!(req.item_limit.unwrap_or(25)),
    );
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
    let table = "alcedo_collection_sections".to_string();
    let service = ItemsService::new(state, ctx, &table);
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
    let table = "alcedo_collection_sections".to_string();
    let service = ItemsService::new(state, ctx, &table);
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
        qtable(ctx, "alcedo_collection_layout_roles"),
        qtable(ctx, "alcedo_roles"),
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
    let mut tx = state.database_pool.begin().await?;
    let delete_sql = format!(
        "DELETE FROM {} WHERE layout_id = '{}'::uuid",
        qtable(ctx, "alcedo_collection_layout_roles"),
        escape_sql_string(layout_id)
    );
    execute_query_transaction(state, &mut tx, &delete_sql).await?;

    if let Some(ids) = body.get("role_ids").and_then(|v| v.as_array()) {
        for role in ids {
            if let Some(role_id) = role.as_str() {
                if uuid::Uuid::parse_str(role_id).is_err() {
                    continue;
                }
                let insert = format!(
                    "INSERT INTO {} (layout_id, role_id) VALUES ('{}'::uuid, '{}'::uuid)",
                    qtable(ctx, "alcedo_collection_layout_roles"),
                    escape_sql_string(layout_id),
                    escape_sql_string(role_id)
                );
                execute_query_transaction(state, &mut tx, &insert).await?;
            }
        }
    }
    tx.commit().await?;
    Ok(json!({ "updated": true }))
}
