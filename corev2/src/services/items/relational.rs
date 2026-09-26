//! Recursive nested relational writes (Directus-style).
//!
//! A nested relationship key is either a relationship **field name** on the
//! source collection, or a **child collection name**. Values may be nested
//! objects/arrays of arbitrary depth. Everything runs inside the caller's
//! transaction.
//!
//! Relations may point at a collection in another app (same version): each
//! relation carries a target app which is used to resolve the target schema
//! (`{app}010{version}`) for reads and nested writes.

use std::collections::HashMap;

use futures::future::BoxFuture;
use serde_json::{Map, Value};
use sqlx::{Postgres, Transaction};

use crate::{
    AppState,
    services::{
        collections::{self, FieldDefinition},
        context::AppContext,
        errors::AlcedoError,
        items::{
            query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query},
            service::ItemsService,
        },
        collections::schema::get_pk_key,
        permissions::{check_write, row_matches_action_sql, WriteGuard},
    },
};

pub enum Direction {
    /// A FK column on the source table points at the target collection.
    ManyToOne {
        target: String,
        target_app: Option<String>,
    },
    /// The target collection has a FK column (`fk`) pointing back at the source.
    OneToMany {
        target: String,
        fk: String,
        target_app: Option<String>,
    },
}

/// Builds the context for a relationship target. Cross-app relations always
/// share the owning collection's version, so only the app changes.
pub(crate) fn target_ctx(ctx: &AppContext, target_app: &Option<String>) -> AppContext {
    AppContext {
        app_name: target_app.clone().unwrap_or_else(|| ctx.app_name.clone()),
        version: ctx.version.clone(),
        request_source: ctx.request_source.clone(),
    }
}

pub(crate) async fn collection_fields(
    state: &AppState,
    ctx: &AppContext,
    collection: &str,
) -> Result<Vec<FieldDefinition>, AlcedoError> {
    Ok(collections::get_collection(state, ctx, collection)
        .await?
        .fields)
}

/// Finds the FK field on `child` pointing back at `parent`.
pub(crate) async fn reverse_fk(
    state: &AppState,
    child_ctx: &AppContext,
    child: &str,
    parent: &str,
) -> Result<Option<String>, AlcedoError> {
    Ok(collection_fields(state, child_ctx, child)
        .await?
        .iter()
        .find(|f| f.is_relationship() && f.related_collection.as_deref() == Some(parent))
        .map(|f| f.name.clone()))
}

pub async fn detect_direction(
    state: &AppState,
    ctx: &AppContext,
    source_fields: &[FieldDefinition],
    source: &str,
    key: &str,
) -> Result<Option<Direction>, AlcedoError> {
    if let Some(field) = source_fields
        .iter()
        .find(|f| f.name == key && f.is_relationship())
    {
        let target = field.related_collection.clone().ok_or_else(|| {
            AlcedoError::InvalidInput(
                format!("Relationship field '{}' has no related_collection", key),
                1,
            )
        })?;
        let target_app = field.related_app.clone();
        if field.is_virtual() {
            let child_ctx = target_ctx(ctx, &target_app);
            let fk = reverse_fk(state, &child_ctx, &target, source)
                .await?
                .ok_or_else(|| {
                    AlcedoError::InvalidInput(
                        format!(
                            "No reverse relationship found on '{}' pointing to '{}'",
                            target, source
                        ),
                        1,
                    )
                })?;
            return Ok(Some(Direction::OneToMany {
                target,
                fk,
                target_app,
            }));
        }
        return Ok(Some(Direction::ManyToOne { target, target_app }));
    }

    // Pass 2: the key may be a child collection name with a reciprocal FK. The
    // child may live in any app attached to the current version.
    let apps: Vec<String> = {
        let schema = state.database_schema.read().await;
        schema
            .app_versions
            .iter()
            .filter(|av| av.version_name == ctx.version_api_name())
            .map(|av| av.app_name.clone())
            .collect()
    };

    let mut candidate: Option<(String, String, String)> = None; // (app, fk, target)
    for app_name in apps {
        let tctx = AppContext {
            app_name: app_name.clone(),
            version: ctx.version.clone(),
            request_source: ctx.request_source.clone(),
        };
        let tables = collections::collection_tables(state, &tctx).await?;
        if !tables.iter().any(|t| t == key) {
            continue;
        }
        if let Some(field) = collection_fields(state, &tctx, key).await?.iter().find(|f| {
            f.is_relationship()
                && f.related_collection.as_deref() == Some(source)
                && !f.is_virtual()
                && f.related_app
                    .as_deref()
                    .map_or(true, |a| a == ctx.app_api_name())
        }) {
            if candidate.is_some() {
                return Err(AlcedoError::InvalidInput(
                    format!(
                        "Ambiguous relation target '{}': it exists in multiple apps",
                        key
                    ),
                    1,
                ));
            }
            candidate = Some((app_name, field.name.clone(), key.to_string()));
        }
    }

    if let Some((app_name, fk, target)) = candidate {
        return Ok(Some(Direction::OneToMany {
            target,
            fk,
            target_app: Some(app_name),
        }));
    }
    Ok(None)
}

fn cmp_in(values: Vec<Value>) -> FieldValue {
    FieldValue::Comparison(Comparison {
        _in: Some(Value::Array(values)),
        ..Default::default()
    })
}

fn cmp_eq(value: Value) -> FieldValue {
    FieldValue::Comparison(Comparison {
        _eq: Some(value),
        ..Default::default()
    })
}

fn query_and(fields: Vec<(&str, FieldValue)>) -> Query {
    let mut map = HashMap::new();
    for (name, value) in fields {
        map.insert(name.to_string(), value);
    }
    Query {
        filter: LogicOp {
            _and: Some(vec![Filter::Field(FieldFilter { fields: map })]),
            _or: None,
        },
        ..Default::default()
    }
}

async fn insert_one(
    state: &AppState,
    ctx: &AppContext,
    tx: &mut Transaction<'_, Postgres>,
    collection: &str,
    item: Map<String, Value>,
) -> Result<String, AlcedoError> {
    let table = collection.to_string();
    let service = ItemsService::new(state, ctx, &table);
    let ids = service.create_many(vec![item], &mut Some(tx)).await?;
    Ok(ids
        .get(0)
        .map(|s| s.to_string())
        .unwrap_or_default())
}

async fn update_one(
    state: &AppState,
    ctx: &AppContext,
    tx: &mut Transaction<'_, Postgres>,
    collection: &str,
    pk: &str,
    scalars: Map<String, Value>,
) -> Result<(), AlcedoError> {
    let pk_name = get_pk_key(&state.database_schema, &ctx.schema_name(), collection)
        .await?
        .name;
    let table = collection.to_string();
    let service = ItemsService::new(state, ctx, &table);
    let mut query = Query::eq(&pk_name, Value::String(pk.to_string()));
    service
        .update_items_by_query(&mut query, scalars, &mut Some(tx))
        .await?;
    Ok(())
}

async fn assign_children(
    state: &AppState,
    ctx: &AppContext,
    tx: &mut Transaction<'_, Postgres>,
    child: &str,
    fk: &str,
    parent_id: &str,
    ids: &[Value],
    guard: Option<&WriteGuard>,
) -> Result<(), AlcedoError> {
    if ids.is_empty() {
        return Ok(());
    }
    if let Some(guard) = guard {
        if let Some(perms) = guard.permissions_for(state, ctx, child, "update").await? {
            let mut payload = Map::new();
            payload.insert(fk.to_string(), Value::String(parent_id.to_string()));
            check_write(&perms, "update", &payload)?;
            // ponytail: one membership query per id; batch with `pk IN (...)`
            // + a count comparison if child batches grow large.
            for id in ids {
                let id = id.as_str().ok_or_else(|| {
                    AlcedoError::InvalidInput("Child ids must be strings".to_string(), 1)
                })?;
                if !row_matches_action_sql(state, ctx, child, &perms, "update", id).await? {
                    return Err(AlcedoError::Forbidden(
                        "Child record is not editable with your permissions".to_string(),
                        0,
                    ));
                }
            }
        }
    }
    let pk_name = get_pk_key(&state.database_schema, &ctx.schema_name(), child)
        .await?
        .name;
    let table = child.to_string();
    let service = ItemsService::new(state, ctx, &table);
    let mut query = query_and(vec![(pk_name.as_str(), cmp_in(ids.to_vec()))]);
    let mut payload = Map::new();
    payload.insert(fk.to_string(), Value::String(parent_id.to_string()));
    service
        .update_items_by_query(&mut query, payload, &mut Some(tx))
        .await?;
    Ok(())
}

async fn unlink_children(
    state: &AppState,
    ctx: &AppContext,
    tx: &mut Transaction<'_, Postgres>,
    child: &str,
    fk: &str,
    parent_id: &str,
    guard: Option<&WriteGuard>,
) -> Result<(), AlcedoError> {
    if let Some(guard) = guard {
        if let Some(perms) = guard.permissions_for(state, ctx, child, "update").await? {
            let mut payload = Map::new();
            payload.insert(fk.to_string(), Value::Null);
            check_write(&perms, "update", &payload)?;

            let pk_name = get_pk_key(&state.database_schema, &ctx.schema_name(), child)
                .await?
                .name;
            let table = child.to_string();
            let service = ItemsService::new(state, ctx, &table);
            let mut existing =
                query_and(vec![(fk, cmp_eq(Value::String(parent_id.to_string())))]);
            existing.fields = vec![pk_name.clone()];
            existing.limit = 0;
            // ponytail: one membership query per child; batch if child batches grow.
            for row in service.read_items_by_query(existing).await? {
                if let Some(id) = row.get(pk_name.as_str()).and_then(Value::as_str) {
                    if !row_matches_action_sql(state, ctx, child, &perms, "update", id).await? {
                        return Err(AlcedoError::Forbidden(
                            "Child record is not editable with your permissions".to_string(),
                            0,
                        ));
                    }
                }
            }
        }
    }
    let table = child.to_string();
    let service = ItemsService::new(state, ctx, &table);
    let mut query = query_and(vec![(fk, cmp_eq(Value::String(parent_id.to_string())))]);
    let mut payload = Map::new();
    payload.insert(fk.to_string(), Value::Null);
    service
        .update_items_by_query(&mut query, payload, &mut Some(tx))
        .await?;
    Ok(())
}

async fn delete_children(
    state: &AppState,
    ctx: &AppContext,
    tx: &mut Transaction<'_, Postgres>,
    child: &str,
    fk: &str,
    parent_id: &str,
    ids: &[Value],
    guard: Option<&WriteGuard>,
) -> Result<(), AlcedoError> {
    if ids.is_empty() {
        return Ok(());
    }
    if let Some(guard) = guard {
        // ponytail: one membership query per id; batch if child batches grow.
        for id in ids {
            let id = id.as_str().ok_or_else(|| {
                AlcedoError::InvalidInput("Child ids must be strings".to_string(), 1)
            })?;
            guard.check_delete_row(state, ctx, child, id).await?;
        }
    }
    let pk_name = get_pk_key(&state.database_schema, &ctx.schema_name(), child)
        .await?
        .name;
    let table = child.to_string();
    let service = ItemsService::new(state, ctx, &table);
    let query = query_and(vec![
        (pk_name.as_str(), cmp_in(ids.to_vec())),
        (fk, cmp_eq(Value::String(parent_id.to_string()))),
    ]);
    service
        .delete_items_by_query(query, &mut Some(tx))
        .await?;
    Ok(())
}

fn collect_create_objects(value: &Value) -> Vec<Map<String, Value>> {
    let array = if value.is_array() {
        value.as_array().cloned().unwrap_or_default()
    } else if let Some(obj) = value.as_object() {
        obj.get("create")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        vec![]
    };
    array
        .into_iter()
        .filter_map(|v| v.as_object().cloned())
        .collect()
}

/// Creates `item` (and any nested relations) in `collection`, returning the
/// created row's primary key. Recurses to unlimited depth. When `guard` is
/// present every target collection's `create`/`update`/`delete` policy is
/// enforced and relationship FKs are verified against the target read policy.
pub fn create_recursive<'a>(
    state: &'a AppState,
    ctx: &'a AppContext,
    tx: &'a mut Transaction<'_, Postgres>,
    collection: String,
    item: Map<String, Value>,
    guard: Option<&'a WriteGuard>,
    injected_fk: Option<String>,
) -> BoxFuture<'a, Result<String, AlcedoError>> {
    Box::pin(async move {
        let fields = collection_fields(state, ctx, &collection).await?;
        let mut scalar = item;
        let mut o2m: Vec<(String, String, Value, Option<String>)> = Vec::new();
        // (target_app, target_collection, id) to verify before insert.
        let mut verify: Vec<(Option<String>, String, String)> = Vec::new();

        // Existing scalar FK ids (not converted by the loop below). The FK
        // injected by the parent link points at a row created in this same
        // transaction, so it is not re-verified.
        for field in &fields {
            if Some(field.name.as_str()) == injected_fk.as_deref() {
                continue;
            }
            if field.is_relationship() && !field.is_virtual() {
                if let (Some(target), Some(Value::String(id))) =
                    (field.related_collection.clone(), scalar.get(&field.name))
                {
                    if !id.is_empty() {
                        verify.push((field.related_app.clone(), target, id.clone()));
                    }
                }
            }
        }

        for key in scalar.keys().cloned().collect::<Vec<_>>() {
            let value = scalar.get(&key).cloned().unwrap_or(Value::Null);
            if !(value.is_object() || value.is_array()) {
                continue;
            }
            match detect_direction(state, ctx, &fields, &collection, &key).await? {
                Some(Direction::ManyToOne { target, target_app }) => {
                    if let Some(obj) = value.as_object() {
                        if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                            scalar.insert(key, Value::String(id.to_string()));
                            verify.push((target_app, target, id.to_string()));
                        } else {
                            let tctx = target_ctx(ctx, &target_app);
                            let child_id = create_recursive(
                                state,
                                &tctx,
                                &mut *tx,
                                target,
                                obj.clone(),
                                guard,
                                None,
                            )
                            .await?;
                            scalar.insert(key, Value::String(child_id));
                        }
                    }
                }
                Some(Direction::OneToMany {
                    target,
                    fk,
                    target_app,
                }) => {
                    o2m.push((target, fk, value, target_app));
                    scalar.remove(&key);
                }
                None => {}
            }
        }

        if let Some(guard) = guard {
            // The parent link FK is server-injected, not user input, so it is
            // exempt from the create field allowlist/validation.
            let validated = match &injected_fk {
                Some(fk) => {
                    let mut body = scalar.clone();
                    body.remove(fk);
                    body
                }
                None => scalar.clone(),
            };
            guard.check_create(state, ctx, &collection, &validated).await?;
            for (target_app, target, id) in &verify {
                let tctx = target_ctx(ctx, target_app);
                guard.verify_reference(state, &tctx, target, id).await?;
            }
        }

        let id = insert_one(state, ctx, &mut *tx, &collection, scalar).await?;

        for (target, fk, value, target_app) in o2m {
            let tctx = target_ctx(ctx, &target_app);
            for mut child in collect_create_objects(&value) {
                child.insert(fk.clone(), Value::String(id.clone()));
                create_recursive(
                    state,
                    &tctx,
                    &mut *tx,
                    target.clone(),
                    child,
                    guard,
                    Some(fk.clone()),
                )
                .await?;
            }
        }
        Ok(id)
    })
}

async fn process_o2m_update(
    state: &AppState,
    ctx: &AppContext,
    tx: &mut Transaction<'_, Postgres>,
    target: &str,
    target_app: &Option<String>,
    fk: &str,
    parent_id: &str,
    value: &Value,
    guard: Option<&WriteGuard>,
) -> Result<(), AlcedoError> {
    let tctx = target_ctx(ctx, target_app);

    if value.is_null() {
        return unlink_children(state, &tctx, tx, target, fk, parent_id, guard).await;
    }

    if let Some(array) = value.as_array() {
        let mut assign_ids: Vec<Value> = Vec::new();
        for element in array {
            if let Some(obj) = element.as_object() {
                let mut child = obj.clone();
                child.insert(fk.to_string(), Value::String(parent_id.to_string()));
                create_recursive(
                    state,
                    &tctx,
                    &mut *tx,
                    target.to_string(),
                    child,
                    guard,
                    Some(fk.to_string()),
                )
                .await?;
            } else if let Some(id) = element.as_str() {
                assign_ids.push(Value::String(id.to_string()));
            } else {
                return Err(AlcedoError::InvalidInput(
                    format!("Invalid element in O2M array for '{}'", target),
                    1,
                ));
            }
        }
        return assign_children(state, &tctx, tx, target, fk, parent_id, &assign_ids, guard).await;
    }

    if let Some(obj) = value.as_object() {
        if obj.get("create").is_none() && obj.get("update").is_none() && obj.get("delete").is_none()
        {
            return Err(AlcedoError::InvalidInput(
                "O2M field object must contain 'create', 'update', or 'delete' keys".to_string(),
                1,
            ));
        }
        if let Some(creates) = obj.get("create").and_then(|v| v.as_array()) {
            for create in creates {
                if let Some(child_obj) = create.as_object() {
                    let mut child = child_obj.clone();
                    child.insert(fk.to_string(), Value::String(parent_id.to_string()));
                    create_recursive(
                        state,
                        &tctx,
                        &mut *tx,
                        target.to_string(),
                        child,
                        guard,
                        Some(fk.to_string()),
                    )
                    .await?;
                }
            }
        }
        if let Some(updates) = obj.get("update").and_then(|v| v.as_array()) {
            for update in updates {
                if let Some(update_obj) = update.as_object() {
                    if let Some(id) = update_obj.get("id").and_then(|v| v.as_str()) {
                        let mut body = update_obj.clone();
                        body.remove("id");
                        update_recursive(
                            state,
                            &tctx,
                            &mut *tx,
                            target.to_string(),
                            id.to_string(),
                            body,
                            guard,
                        )
                        .await?;
                    }
                }
            }
        }
        if let Some(deletes) = obj.get("delete").and_then(|v| v.as_array()) {
            let ids: Vec<Value> = deletes
                .iter()
                .filter_map(|v| v.as_str().map(|s| Value::String(s.to_string())))
                .collect();
            delete_children(state, &tctx, tx, target, fk, parent_id, &ids, guard).await?;
        }
        return Ok(());
    }

    Err(AlcedoError::InvalidInput(
        format!(
            "Invalid value type for O2M field on '{}': expected object, array, or null",
            target
        ),
        1,
    ))
}

/// Updates the row `id` in `collection` plus any nested relations, to unlimited
/// depth. `body` keys that are not nested relationships are written to the row.
pub fn update_recursive<'a>(
    state: &'a AppState,
    ctx: &'a AppContext,
    tx: &'a mut Transaction<'_, Postgres>,
    collection: String,
    id: String,
    body: Map<String, Value>,
    guard: Option<&'a WriteGuard>,
) -> BoxFuture<'a, Result<(), AlcedoError>> {
    Box::pin(async move {
        let fields = collection_fields(state, ctx, &collection).await?;
        let mut scalar = Map::new();
        let mut o2m: Vec<(String, String, Value, Option<String>)> = Vec::new();
        let mut verify: Vec<(Option<String>, String, String)> = Vec::new();

        for (key, value) in body {
            let direction = if value.is_object() || value.is_array() || value.is_null() {
                detect_direction(state, ctx, &fields, &collection, &key).await?
            } else {
                None
            };

            match direction {
                Some(Direction::ManyToOne { target, target_app }) => {
                    if value.is_null() {
                        scalar.insert(key, Value::Null);
                    } else if let Some(obj) = value.as_object() {
                        let tctx = target_ctx(ctx, &target_app);
                        if let Some(related_id) = obj.get("id").and_then(|v| v.as_str()) {
                            let mut child_body = obj.clone();
                            child_body.remove("id");
                            update_recursive(
                                state,
                                &tctx,
                                &mut *tx,
                                target,
                                related_id.to_string(),
                                child_body,
                                guard,
                            )
                            .await?;
                            // FK is unchanged.
                        } else {
                            let new_id = create_recursive(
                                state,
                                &tctx,
                                &mut *tx,
                                target,
                                obj.clone(),
                                guard,
                                None,
                            )
                            .await?;
                            scalar.insert(key, Value::String(new_id));
                        }
                    } else if let Some(s) = value.as_str() {
                        scalar.insert(key, Value::String(s.to_string()));
                        verify.push((target_app, target, s.to_string()));
                    }
                }
                Some(Direction::OneToMany {
                    target,
                    fk,
                    target_app,
                }) => {
                    o2m.push((target, fk, value, target_app));
                }
                None => {
                    scalar.insert(key, value);
                }
            }
        }

        if let Some(guard) = guard {
            guard
                .check_update_row(state, ctx, &collection, &id, &scalar)
                .await?;
            for (target_app, target, ref_id) in &verify {
                let tctx = target_ctx(ctx, target_app);
                guard.verify_reference(state, &tctx, target, ref_id).await?;
            }
        }

        for (target, fk, value, target_app) in o2m {
            process_o2m_update(
                state, ctx, &mut *tx, &target, &target_app, &fk, &id, &value, guard,
            )
            .await?;
        }

        if !scalar.is_empty() {
            update_one(state, ctx, &mut *tx, &collection, &id, scalar).await?;
        }
        Ok(())
    })
}
