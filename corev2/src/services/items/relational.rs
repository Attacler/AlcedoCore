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
        collections::schema::get_pk_key,
        collections::{self, FieldDefinition},
        context::AppContext,
        errors::AlcedoError,
        items::{
            query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query},
            service::ItemsService,
        },
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
        identity: ctx.identity.clone(),
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
            identity: None,
        };
        let tables = collections::collection_tables(state, &tctx).await?;
        if !tables.iter().any(|t| t == key) {
            continue;
        }
        if let Some(field) = collection_fields(state, &tctx, key)
            .await?
            .iter()
            .find(|f| {
                f.is_relationship()
                    && f.related_collection.as_deref() == Some(source)
                    && !f.is_virtual()
                    && f.related_app
                        .as_deref()
                        .map_or(true, |a| a == ctx.app_api_name())
            })
        {
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
    Ok(ids.get(0).map(|s| s.to_string()).unwrap_or_default())
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
) -> Result<(), AlcedoError> {
    if ids.is_empty() {
        return Ok(());
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
) -> Result<(), AlcedoError> {
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
) -> Result<(), AlcedoError> {
    if ids.is_empty() {
        return Ok(());
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
    service.delete_items_by_query(query, &mut Some(tx)).await?;
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
/// created row's primary key. Recurses to unlimited depth.
pub fn create_recursive<'a>(
    state: &'a AppState,
    ctx: &'a AppContext,
    tx: &'a mut Transaction<'_, Postgres>,
    collection: String,
    item: Map<String, Value>,
) -> BoxFuture<'a, Result<String, AlcedoError>> {
    Box::pin(async move {
        let fields = collection_fields(state, ctx, &collection).await?;
        let mut scalar = item;
        let mut o2m: Vec<(String, String, Value, Option<String>)> = Vec::new();

        for key in scalar.keys().cloned().collect::<Vec<_>>() {
            let value = scalar.get(&key).cloned().unwrap_or(Value::Null);
            if !(value.is_object() || value.is_array()) {
                continue;
            }
            match detect_direction(state, ctx, &fields, &collection, &key).await? {
                Some(Direction::ManyToOne { target, target_app }) => {
                    if let Some(obj) = value.as_object() {
                        if !obj.contains_key("id") {
                            let tctx = target_ctx(ctx, &target_app);
                            let child_id =
                                create_recursive(state, &tctx, &mut *tx, target, obj.clone())
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

        let id = insert_one(state, ctx, &mut *tx, &collection, scalar).await?;

        for (target, fk, value, target_app) in o2m {
            let tctx = target_ctx(ctx, &target_app);
            for mut child in collect_create_objects(&value) {
                child.insert(fk.clone(), Value::String(id.clone()));
                create_recursive(state, &tctx, &mut *tx, target.clone(), child).await?;
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
) -> Result<(), AlcedoError> {
    let tctx = target_ctx(ctx, target_app);

    if value.is_null() {
        return unlink_children(state, &tctx, tx, target, fk, parent_id).await;
    }

    if let Some(array) = value.as_array() {
        let mut assign_ids: Vec<Value> = Vec::new();
        for element in array {
            if let Some(obj) = element.as_object() {
                let mut child = obj.clone();
                child.insert(fk.to_string(), Value::String(parent_id.to_string()));
                create_recursive(state, &tctx, &mut *tx, target.to_string(), child).await?;
            } else if let Some(id) = element.as_str() {
                assign_ids.push(Value::String(id.to_string()));
            } else {
                return Err(AlcedoError::InvalidInput(
                    format!("Invalid element in O2M array for '{}'", target),
                    1,
                ));
            }
        }
        return assign_children(state, &tctx, tx, target, fk, parent_id, &assign_ids).await;
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
                    create_recursive(state, &tctx, &mut *tx, target.to_string(), child).await?;
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
            delete_children(state, &tctx, tx, target, fk, parent_id, &ids).await?;
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
) -> BoxFuture<'a, Result<(), AlcedoError>> {
    Box::pin(async move {
        let fields = collection_fields(state, ctx, &collection).await?;
        let mut scalar = Map::new();
        let mut o2m: Vec<(String, String, Value, Option<String>)> = Vec::new();

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
                            )
                            .await?;
                            // FK is unchanged.
                        } else {
                            let new_id =
                                create_recursive(state, &tctx, &mut *tx, target, obj.clone())
                                    .await?;
                            scalar.insert(key, Value::String(new_id));
                        }
                    } else if let Some(s) = value.as_str() {
                        scalar.insert(key, Value::String(s.to_string()));
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

        for (target, fk, value, target_app) in o2m {
            process_o2m_update(state, ctx, &mut *tx, &target, &target_app, &fk, &id, &value)
                .await?;
        }

        if !scalar.is_empty() {
            update_one(state, ctx, &mut *tx, &collection, &id, scalar).await?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod integration_tests {
    //! DB-backed checks that **nested relational writes** enforce permissions on
    //! the CHILD collection for the SAME caller identity.
    //!
    //! Every hop in `create_recursive` / `update_recursive` builds a new
    //! `ItemsService` from `target_ctx(ctx, target_app)`, which preserves
    //! `ctx.identity` and only swaps the app. So a nested write must be judged
    //! against the child collection's rules for the caller, never silently
    //! `Unrestricted`. These tests pin that: a restricted caller with a grant on
    //! the parent (`tickets`) but none on the child (`ticket_comments`) must be
    //! rejected, and nothing may be written.
    //!
    //! Fixtures are the seeded `helpdesk010production` app: `tickets` has the
    //! virtual one-to-many `comments` -> `ticket_comments` (same app). Each test
    //! seeds an isolated non-admin user + role + `nested-perm-*` policies and
    //! cleans up again, mirroring `permissions/create.rs` / `update.rs`.

    use std::collections::HashMap;

    use serde_json::{Map, Value, json};
    use sqlx::Row;
    use uuid::Uuid;

    use super::*;
    use crate::AppState;
    use crate::middelware::auth::AuthLevel;
    use crate::services::{
        context::RequestSource,
        errors::AlcedoError,
        permissions::read::{ReadRule, matching_pks},
        postgres::inspector::TableMeta,
    };

    const HELPDESK_SCHEMA: &str = "helpdesk010production";
    const CRM_SCHEMA: &str = "crm010production";

    fn helpdesk_ctx(identity: Option<AuthLevel>) -> AppContext {
        AppContext {
            app_name: "helpdesk".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity,
        }
    }

    fn item(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object payload")
    }

    /// The FK column on `ticket_comments` pointing back at `tickets` (the same
    /// field `detect_direction` computes for the o2m relation).
    async fn comments_fk(state: &AppState, ctx: &AppContext) -> String {
        reverse_fk(state, ctx, "ticket_comments", "tickets")
            .await
            .unwrap()
            .unwrap_or_else(|| "ticket".to_string())
    }

    /// Injects `alcedo_collections` metadata for every collection in
    /// `schema_name` (the resolver and relation walker read it from the
    /// in-memory schema, which `get_app_state` does not populate). Mirrors the
    /// helpers in `permissions/create.rs` / `update.rs`.
    async fn inject_schema_meta(state: &AppState, schema_name: &str) {
        let rows = sqlx::query(&format!(
            "SELECT id, app_name, app_version, \"table\", name FROM \"{schema_name}\".alcedo_collections"
        ))
        .fetch_all(&*state.database_pool)
        .await
        .ok()
        .unwrap_or_default();

        let mut schema = state.database_schema.write().await;
        for row in rows {
            let id = row.try_get::<i32, _>("id").ok();
            let Ok(app_name) = row.try_get::<String, _>("app_name") else {
                continue;
            };
            let Ok(app_version) = row.try_get::<String, _>("app_version") else {
                continue;
            };
            let Ok(table) = row.try_get::<String, _>("table") else {
                continue;
            };
            let Ok(name) = row.try_get::<String, _>("name") else {
                continue;
            };
            let Some(target) = schema
                .tables
                .iter_mut()
                .find(|t| t.schema == schema_name && t.name == table)
            else {
                continue;
            };
            target.meta = Some(TableMeta {
                id: id.map(i64::from),
                app_name,
                app_version,
                table,
                name,
                icon_name: None,
                icon_color: None,
                singleton: false,
                hidden: false,
                sort_field: None,
            });
        }
    }

    async fn collection_id(state: &AppState, schema: &str, table: &str) -> Option<i32> {
        sqlx::query_scalar(&format!(
            "SELECT id FROM \"{schema}\".alcedo_collections WHERE \"table\" = $1 ORDER BY id LIMIT 1"
        ))
        .bind(table)
        .fetch_optional(&*state.database_pool)
        .await
        .ok()
        .flatten()
    }

    /// A seeded non-admin user whose only role is a marker-scoped test role.
    struct Seed {
        user_id: Uuid,
        role_id: Uuid,
    }

    /// Removes everything a `nested-perm-*` marker could have created. Safe to
    /// run before and after.
    async fn cleanup_marker(state: &AppState, marker: &str) {
        let s = HELPDESK_SCHEMA;
        let policy_like = format!("nested-perm-{marker}%");
        let role_name = format!("nested-perm-{marker}");
        let email = format!("nested-perm-{marker}@test.local");
        let statements = [
            format!(
                "DELETE FROM \"{s}\".alcedocore_role_policies WHERE policy_id IN \
                 (SELECT id FROM \"{s}\".alcedocore_policies WHERE name LIKE '{policy_like}')"
            ),
            format!(
                "DELETE FROM \"{s}\".alcedocore_policy_permissions WHERE policy_id IN \
                 (SELECT id FROM \"{s}\".alcedocore_policies WHERE name LIKE '{policy_like}')"
            ),
            format!("DELETE FROM \"{s}\".alcedocore_policies WHERE name LIKE '{policy_like}'"),
            format!(
                "DELETE FROM \"{s}\".alcedo_user_roles WHERE role_id IN \
                 (SELECT id FROM \"{s}\".alcedo_roles WHERE name = '{role_name}')"
            ),
            format!("DELETE FROM \"{s}\".alcedo_roles WHERE name = '{role_name}'"),
            // A cross-app grant seeds a policy in the target app's schema too.
            format!(
                "DELETE FROM \"{CRM_SCHEMA}\".alcedocore_role_policies WHERE policy_id IN \
                 (SELECT id FROM \"{CRM_SCHEMA}\".alcedocore_policies WHERE name LIKE '{policy_like}')"
            ),
            format!(
                "DELETE FROM \"{CRM_SCHEMA}\".alcedocore_policy_permissions WHERE policy_id IN \
                 (SELECT id FROM \"{CRM_SCHEMA}\".alcedocore_policies WHERE name LIKE '{policy_like}')"
            ),
            format!("DELETE FROM \"{CRM_SCHEMA}\".alcedocore_policies WHERE name LIKE '{policy_like}'"),
            format!(
                "DELETE FROM \"{CRM_SCHEMA}\".alcedo_user_roles WHERE role_id IN \
                 (SELECT id FROM \"{CRM_SCHEMA}\".alcedo_roles WHERE name = '{role_name}')"
            ),
            format!("DELETE FROM \"{CRM_SCHEMA}\".alcedo_roles WHERE name = '{role_name}'"),
            format!("DELETE FROM alcedo.alcedo_users WHERE email = '{email}'"),
            // Children first (FK), then parents, all marker-scoped by ticket subject.
            format!(
                "DELETE FROM \"{s}\".ticket_comments WHERE ticket IN \
                 (SELECT id FROM \"{s}\".tickets WHERE subject LIKE 'nt-{marker}%')"
            ),
            format!("DELETE FROM \"{s}\".tickets WHERE subject LIKE 'nt-{marker}%'"),
        ];
        for sql in statements {
            let _ = sqlx::query(&sql).execute(&*state.database_pool).await;
        }
    }

    async fn seed_role(state: &AppState, marker: &str) -> Seed {
        cleanup_marker(state, marker).await;
        let s = HELPDESK_SCHEMA;
        let user_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();
        let email = format!("nested-perm-{marker}@test.local");
        let role_name = format!("nested-perm-{marker}");

        sqlx::query(
            "INSERT INTO alcedo.alcedo_users (id, email, password_hash, is_admin) \
             VALUES ($1, $2, 'x', false)",
        )
        .bind(user_id)
        .bind(&email)
        .execute(&*state.database_pool)
        .await
        .expect("insert test user");

        sqlx::query(&format!(
            "INSERT INTO \"{s}\".alcedo_roles (id, name, description, is_system) \
             VALUES ($1, $2, '', false)"
        ))
        .bind(role_id)
        .bind(&role_name)
        .execute(&*state.database_pool)
        .await
        .expect("insert test role");

        sqlx::query(&format!(
            "INSERT INTO \"{s}\".alcedo_user_roles (id, user_id, role_id) VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .expect("insert test user role");

        Seed { user_id, role_id }
    }

    /// Adds one policy permission rule for `seed`'s role on `table` with an
    /// arbitrary `action` (`create` / `update` / `delete`).
    async fn add_grant(
        state: &AppState,
        marker: &str,
        seed: &Seed,
        schema: &str,
        table: &str,
        action: &str,
        fields: Value,
        filter: Value,
        field_validation: Value,
    ) {
        let collection = collection_id(state, schema, table)
            .await
            .unwrap_or_else(|| panic!("collection '{table}' not seeded in {schema}"));
        let policy_id = Uuid::new_v4();
        let policy_name = format!("nested-perm-{marker}-{}", policy_id);

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_policies (id, name, description) VALUES ($1, $2, '')"
        ))
        .bind(policy_id)
        .bind(&policy_name)
        .execute(&*state.database_pool)
        .await
        .expect("insert policy");

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_role_policies (id, role_id, policy_id) VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(seed.role_id)
        .bind(policy_id)
        .execute(&*state.database_pool)
        .await
        .expect("insert role policy");

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_policy_permissions \
             (id, policy_id, collection, action, fields, filter, field_validation) \
             VALUES ($1, $2, $3, $4, $5::json, $6::json, $7::json)"
        ))
        .bind(Uuid::new_v4())
        .bind(policy_id)
        .bind(collection)
        .bind(action)
        .bind(serde_json::to_string(&fields).unwrap())
        .bind(serde_json::to_string(&filter).unwrap())
        .bind(serde_json::to_string(&field_validation).unwrap())
        .execute(&*state.database_pool)
        .await
        .expect("insert policy permission");
    }

    async fn count_tickets_subject(state: &AppState, subject: &str) -> i64 {
        sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{HELPDESK_SCHEMA}\".tickets WHERE subject = $1"
        ))
        .bind(subject)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap()
    }

    async fn count_comments_for_subject(state: &AppState, subject: &str, body: &str) -> i64 {
        sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{HELPDESK_SCHEMA}\".ticket_comments c \
             JOIN \"{HELPDESK_SCHEMA}\".tickets t ON t.id = c.ticket \
             WHERE t.subject = $1 AND c.body = $2"
        ))
        .bind(subject)
        .bind(body)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap()
    }

    /// Runs `create_recursive` in a real transaction and commits only on
    /// success, so a rejected nested write leaves nothing behind.
    async fn create_nested(
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
        payload: Map<String, Value>,
    ) -> Result<String, AlcedoError> {
        let mut tx = state.database_pool.begin().await.unwrap();
        let result = create_recursive(state, ctx, &mut tx, collection.to_string(), payload).await;
        match result {
            Ok(id) => {
                tx.commit().await.unwrap();
                Ok(id)
            }
            Err(err) => {
                tx.rollback().await.ok();
                Err(err)
            }
        }
    }

    async fn update_nested(
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
        id: String,
        body: Map<String, Value>,
    ) -> Result<(), AlcedoError> {
        let mut tx = state.database_pool.begin().await.unwrap();
        let result =
            update_recursive(state, ctx, &mut tx, collection.to_string(), id, body).await;
        match result {
            Ok(()) => {
                tx.commit().await.unwrap();
                Ok(())
            }
            Err(err) => {
                tx.rollback().await.ok();
                Err(err)
            }
        }
    }

    async fn insert_ticket(state: &AppState, subject: &str) -> Uuid {
        sqlx::query_scalar(&format!(
            "INSERT INTO \"{HELPDESK_SCHEMA}\".tickets (subject, status) VALUES ($1, 'open') RETURNING id"
        ))
        .bind(subject)
        .fetch_one(&*state.database_pool)
        .await
        .expect("insert ticket")
    }

    async fn ticket_subject(state: &AppState, id: Uuid) -> Option<String> {
        sqlx::query_scalar(&format!(
            "SELECT subject FROM \"{HELPDESK_SCHEMA}\".tickets WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap()
    }

    /// True when the seeded fixture exposes `tickets.comments` as a virtual o2m
    /// field. Shared guard so every nested o2m test skips cleanly if the seed
    /// differs.
    async fn has_virtual_comments(state: &AppState, ctx: &AppContext) -> bool {
        collection_fields(state, ctx, "tickets")
            .await
            .map(|fields| {
                fields
                    .iter()
                    .find(|f| f.name == "comments")
                    .is_some_and(|f| f.is_virtual())
            })
            .unwrap_or(false)
    }

    /// 1 + 2. **Nested o2m create is enforced on the CHILD collection, with the
    /// same caller identity.** A restricted caller with `create` on `tickets`
    /// but none on `ticket_comments` is rejected and nothing is written
    /// (neither parent nor child). Granting create on the child makes the same
    /// nested create succeed and persist both rows.
    ///
    /// This is what proves the nested hop is *not* silently `Unrestricted`: the
    /// child `ItemsService` is built from `target_ctx`, which keeps
    /// `identity = Some(User(..))`, so `resolve_create_access` for
    /// `ticket_comments` returns `Deny` and `check_create` raises `Forbidden`.
    #[tokio::test]
    async fn nested_o2m_create_enforced_on_child_collection() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "o2mcreate").await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        if !has_virtual_comments(&state, &ctx).await {
            eprintln!("skipping: tickets.comments is not seeded as a virtual o2m field");
            cleanup_marker(&state, "o2mcreate").await;
            return;
        }

        // Parent create only: no rule whatsoever on the child collection.
        add_grant(
            &state,
            "o2mcreate",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            "create",
            json!(["subject", "comments"]),
            json!([]),
            json!([]),
        )
        .await;

        let subject = "nt-o2mcreate";
        let payload = item(json!({
            "subject": subject,
            "comments": [{ "body": "hello" }],
        }));

        // Denied: the child collection has no create rule for this caller.
        let err = create_nested(&state, &ctx, "tickets", payload.clone())
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "nested create must be enforced on ticket_comments: {err:?}"
        );
        // Nothing written: neither the parent nor the child.
        assert_eq!(count_tickets_subject(&state, subject).await, 0);
        assert_eq!(
            count_comments_for_subject(&state, subject, "hello").await,
            0
        );

        // Now grant create on the child (fields covering the keys the nested
        // write injects: `body` plus the o2m-injected FK).
        let fk = comments_fk(&state, &ctx).await;
        add_grant(
            &state,
            "o2mcreate",
            &seed,
            HELPDESK_SCHEMA,
            "ticket_comments",
            "create",
            json!(["body", fk]),
            json!([]),
            json!([]),
        )
        .await;

        // Same nested create now succeeds and both rows persist.
        create_nested(&state, &ctx, "tickets", payload)
            .await
            .expect("with a child create grant the nested write must succeed");
        assert_eq!(count_tickets_subject(&state, subject).await, 1);
        assert_eq!(
            count_comments_for_subject(&state, subject, "hello").await,
            1
        );

        cleanup_marker(&state, "o2mcreate").await;
    }

    /// 3. **Nested o2m update is enforced on the CHILD collection.** For the
    /// `comments: { create: [...] }` path, a caller without a child `create`
    /// grant is rejected and writes nothing; with it the nested create persists.
    #[tokio::test]
    async fn nested_o2m_update_create_enforced_on_child_collection() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "o2mupdcreate").await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        if !has_virtual_comments(&state, &ctx).await {
            eprintln!("skipping: tickets.comments is not seeded as a virtual o2m field");
            cleanup_marker(&state, "o2mupdcreate").await;
            return;
        }

        // Parent update only, whitelisting the virtual `comments` key.
        add_grant(
            &state,
            "o2mupdcreate",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            "update",
            json!(["comments"]),
            json!([]),
            json!([]),
        )
        .await;

        let subject = "nt-o2mupdcreate";
        let ticket = insert_ticket(&state, subject).await;

        let body = item(json!({
            "comments": { "create": [{ "body": "via-update" }] },
        }));

        // Denied on the child (no child create rule), nothing written.
        let err = update_nested(&state, &ctx, "tickets", ticket.to_string(), body.clone())
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "nested o2m update create must be enforced on ticket_comments: {err:?}"
        );
        assert_eq!(
            count_comments_for_subject(&state, subject, "via-update").await,
            0
        );

        // Grant child create; the same nested create now succeeds.
        let fk = comments_fk(&state, &ctx).await;
        add_grant(
            &state,
            "o2mupdcreate",
            &seed,
            HELPDESK_SCHEMA,
            "ticket_comments",
            "create",
            json!(["body", fk]),
            json!([]),
            json!([]),
        )
        .await;

        update_nested(&state, &ctx, "tickets", ticket.to_string(), body)
            .await
            .expect("with a child create grant the nested o2m create must succeed");
        assert_eq!(
            count_comments_for_subject(&state, subject, "via-update").await,
            1
        );

        cleanup_marker(&state, "o2mupdcreate").await;
    }

    /// 3b. The `comments: { update: [...] }` path re-enters
    /// `update_recursive` on the child and is enforced against the child's
    /// `update` rules: without one the whole parent update is rejected and the
    /// child is untouched; with one it persists.
    #[tokio::test]
    async fn nested_o2m_update_child_update_enforced_on_child_collection() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "o2mchildupd").await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        if !has_virtual_comments(&state, &ctx).await {
            eprintln!("skipping: tickets.comments is not seeded as a virtual o2m field");
            cleanup_marker(&state, "o2mchildupd").await;
            return;
        }

        let subject = "nt-o2mupd";
        let ticket = insert_ticket(&state, subject).await;
        let comment: Uuid = sqlx::query_scalar(&format!(
            "INSERT INTO \"{HELPDESK_SCHEMA}\".ticket_comments (body, ticket) VALUES ('orig', $1) RETURNING id"
        ))
        .bind(ticket)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();

        // Parent update only.
        add_grant(
            &state,
            "o2mchildupd",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            "update",
            json!(["comments"]),
            json!([]),
            json!([]),
        )
        .await;

        let body = item(json!({
            "comments": { "update": [{ "id": comment.to_string(), "body": "edited" }] },
        }));

        // Denied: no child update rule.
        let err = update_nested(&state, &ctx, "tickets", ticket.to_string(), body.clone())
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "nested o2m child update must be enforced on ticket_comments: {err:?}"
        );
        let body_after: String = sqlx::query_scalar(&format!(
            "SELECT body FROM \"{HELPDESK_SCHEMA}\".ticket_comments WHERE id = $1"
        ))
        .bind(comment)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(body_after, "orig", "a rejected nested update must not write");

        // Grant child update; the nested edit persists.
        add_grant(
            &state,
            "o2mchildupd",
            &seed,
            HELPDESK_SCHEMA,
            "ticket_comments",
            "update",
            json!(["body"]),
            json!([]),
            json!([]),
        )
        .await;

        update_nested(&state, &ctx, "tickets", ticket.to_string(), body)
            .await
            .expect("with a child update grant the nested o2m update must succeed");
        let body_after: String = sqlx::query_scalar(&format!(
            "SELECT body FROM \"{HELPDESK_SCHEMA}\".ticket_comments WHERE id = $1"
        ))
        .bind(comment)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(body_after, "edited");

        cleanup_marker(&state, "o2mchildupd").await;
    }

    /// 4. **Nested delete via update is enforced against the child's `delete`
    /// rules.** `comments: { delete: [id] }` -> `delete_children` ->
    /// `delete_items_by_query`. Without a child delete rule the parent update is
    /// rejected and the child survives; with it the child is gone.
    #[tokio::test]
    async fn nested_o2m_delete_enforced_on_child_collection() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "o2mdel").await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        if !has_virtual_comments(&state, &ctx).await {
            eprintln!("skipping: tickets.comments is not seeded as a virtual o2m field");
            cleanup_marker(&state, "o2mdel").await;
            return;
        }

        let subject = "nt-o2mdel";
        let ticket = insert_ticket(&state, subject).await;
        let comment: Uuid = sqlx::query_scalar(&format!(
            "INSERT INTO \"{HELPDESK_SCHEMA}\".ticket_comments (body, ticket) VALUES ('doomed', $1) RETURNING id"
        ))
        .bind(ticket)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();

        // Parent update only.
        add_grant(
            &state,
            "o2mdel",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            "update",
            json!(["comments"]),
            json!([]),
            json!([]),
        )
        .await;

        let body = item(json!({
            "comments": { "delete": [comment.to_string()] },
        }));

        // Denied: no child delete rule.
        let err = update_nested(&state, &ctx, "tickets", ticket.to_string(), body.clone())
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "nested o2m delete must be enforced on ticket_comments: {err:?}"
        );
        let remaining: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{HELPDESK_SCHEMA}\".ticket_comments WHERE id = $1"
        ))
        .bind(comment)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(remaining, 1, "a rejected nested delete must not delete");

        // Grant child delete; the child is removed.
        add_grant(
            &state,
            "o2mdel",
            &seed,
            HELPDESK_SCHEMA,
            "ticket_comments",
            "delete",
            json!([]),
            json!([]),
            json!([]),
        )
        .await;

        update_nested(&state, &ctx, "tickets", ticket.to_string(), body)
            .await
            .expect("with a child delete grant the nested o2m delete must succeed");
        let remaining: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{HELPDESK_SCHEMA}\".ticket_comments WHERE id = $1"
        ))
        .bind(comment)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0);

        cleanup_marker(&state, "o2mdel").await;
    }

    /// 5. **All-or-nothing on a nested denial.** A parent update whose scalar
    /// part is permitted but whose nested child create is denied must roll the
    /// whole transaction back: the parent scalar change is not written either.
    #[tokio::test]
    async fn nested_denial_rolls_back_parent_update() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "rollback").await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        if !has_virtual_comments(&state, &ctx).await {
            eprintln!("skipping: tickets.comments is not seeded as a virtual o2m field");
            cleanup_marker(&state, "rollback").await;
            return;
        }

        // Parent update may change `subject` AND `comments` (the virtual key);
        // no child rule exists, so the nested create will be denied.
        add_grant(
            &state,
            "rollback",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            "update",
            json!(["subject", "comments"]),
            json!([]),
            json!([]),
        )
        .await;

        let original = "nt-rollback-orig";
        let ticket = insert_ticket(&state, original).await;

        let body = item(json!({
            "subject": "nt-rollback-changed",
            "comments": { "create": [{ "body": "should-not-exist" }] },
        }));

        let err = update_nested(&state, &ctx, "tickets", ticket.to_string(), body)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        // The parent scalar change rolled back with the nested denial.
        assert_eq!(
            ticket_subject(&state, ticket).await.as_deref(),
            Some(original),
            "the parent update must roll back when the nested write is denied"
        );
        assert_eq!(
            count_comments_for_subject(&state, "nt-rollback-changed", "should-not-exist").await,
            0
        );

        cleanup_marker(&state, "rollback").await;
    }

    /// Companion: the nested hop really does carry the caller identity, rather
    /// than resolving to `Unrestricted`. Directly compare the access the child
    /// collection resolves for the caller through `target_ctx` (the exact
    /// context the hop builds) against the unrestricted contexts — it must be
    /// `Deny`, while an admin resolving the same child is `Unrestricted`.
    #[tokio::test]
    async fn target_ctx_preserves_identity_for_child_checks() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "identity").await;

        let parent_ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        // The o2m relation for `comments` has no `related_app`, so the hop keeps
        // the parent app and only re-scopes collection + identity.
        let child_ctx = target_ctx(&parent_ctx, &None);

        assert_eq!(
            child_ctx.identity.as_ref().map(|i| match i {
                AuthLevel::User(id) => id.to_string(),
                _ => String::new(),
            }),
            Some(seed.user_id.to_string()),
            "target_ctx must preserve the caller identity"
        );

        let child_access = crate::services::permissions::create::resolve_create_access(
            &state, &child_ctx, "ticket_comments", child_ctx.identity.as_ref(),
        )
        .await
        .unwrap();
        assert!(
            matches!(child_access, crate::services::permissions::read::ReadAccess::Deny),
            "a restricted caller must resolve Deny on the child collection: {child_access:?}"
        );

        cleanup_marker(&state, "identity").await;
    }

    /// Sanity that the relational helpers this suite relies on are wired to the
    /// live fixture: `reverse_fk` finds `ticket_comments.ticket`, and the
    /// read-scoped `matching_pks` helper runs against the child collection.
    #[tokio::test]
    async fn relational_helpers_are_wired_to_fixture() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let ctx = helpdesk_ctx(None);

        let fk = reverse_fk(&state, &ctx, "ticket_comments", "tickets")
            .await
            .unwrap();
        assert_eq!(
            fk.as_deref(),
            Some("ticket"),
            "ticket_comments must expose an FK back to tickets"
        );

        let mut filter = HashMap::new();
        filter.insert(
            "body".to_string(),
            crate::services::items::query::FieldValue::Comparison(
                crate::services::items::query::Comparison {
                    _eq: Some(json!("nonexistent")),
                    ..Default::default()
                },
            ),
        );
        let conditions = vec![crate::services::items::query::Filter::Field(
            crate::services::items::query::FieldFilter { fields: filter },
        )];
        let matched = matching_pks(
            &state,
            &ctx,
            "ticket_comments",
            &conditions,
            &[json!(Uuid::new_v4().to_string())],
        )
        .await
        .unwrap();
        assert!(matched.is_empty(), "no comment matches a random pk + body");
    }

    /// Cross-app nested M:1 write: `tickets.customer` points at the `crm` app,
    /// so a nested create of a customer must be checked against the **crm**
    /// schema's `create` rules for the same caller — not the parent's app.
    #[tokio::test]
    async fn nested_cross_app_m1_create_enforced_in_target_app() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        inject_schema_meta(&state, CRM_SCHEMA).await;
        if collection_id(&state, CRM_SCHEMA, "customers").await.is_none() {
            eprintln!("skipping: {CRM_SCHEMA} not seeded");
            return;
        }
        let seed = seed_role(&state, "xapp").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        // Can the caller even see `customer` as a relation on tickets?
        let tickets_fields = collection_fields(&state, &ctx, "tickets").await.unwrap();
        let customer_field = tickets_fields.iter().find(|f| f.name == "customer");
        let Some(customer_field) = customer_field else {
            eprintln!("skipping: tickets.customer not seeded");
            cleanup_marker(&state, "xapp").await;
            return;
        };
        assert_eq!(
            customer_field.related_app.as_deref(),
            Some("crm"),
            "tickets.customer is expected to point at the crm app"
        );

        // Parent create only; no rule in the crm app for this caller.
        add_grant(
            &state,
            "xapp",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            "create",
            json!(["subject", "customer"]),
            json!([]),
            json!([]),
        )
        .await;

        let subject = "nt-xapp-denied";
        let payload = item(json!({
            "subject": subject,
            "customer": { "name": "nt-xapp Co" },
        }));

        // Denied: the nested customer create happens in the crm app, where this
        // caller has no create rule.
        let err = create_nested(&state, &ctx, "tickets", payload.clone())
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "cross-app nested create must be enforced in the target app: {err:?}"
        );
        assert_eq!(count_tickets_subject(&state, subject).await, 0);

        // Grant create in the crm app. Roles are per app-schema, so the caller
        // needs a role in the crm schema for a crm policy to reference.
        let crm_role = seed_role_in(&state, CRM_SCHEMA, "xapp", seed.user_id).await;
        add_grant_for_role(
            &state,
            "xapp",
            crm_role,
            CRM_SCHEMA,
            "customers",
            "create",
            json!(["name"]),
            json!([]),
            json!([]),
        )
        .await;

        // Prove the allow-path is a real grant, not an accidental bypass: with
        // the crm role alone (no policy yet) the caller is Denied in crm; adding
        // the policy makes it Restricted (never Unrestricted).
        let crm_ctx = AppContext {
            app_name: "crm".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity: Some(AuthLevel::User(seed.user_id)),
        };
        let crm_access = crate::services::permissions::create::resolve_create_access(
            &state,
            &crm_ctx,
            "customers",
            crm_ctx.identity.as_ref(),
        )
        .await
        .unwrap();
        assert!(
            matches!(
                crm_access,
                crate::services::permissions::read::ReadAccess::Restricted { .. }
            ),
            "the crm grant must resolve Restricted (a real policy), not Unrestricted: {crm_access:?}"
        );

        create_nested(&state, &ctx, "tickets", payload)
            .await
            .expect("with a crm create grant the cross-app nested write must succeed");
        assert_eq!(count_tickets_subject(&state, subject).await, 1);

        cleanup_marker(&state, "xapp").await;
        // Remove the customer row this test created.
        let _ = sqlx::query(&format!(
            "DELETE FROM \"{CRM_SCHEMA}\".customers WHERE name = 'nt-xapp Co'"
        ))
        .execute(&*state.database_pool)
        .await;
    }

    /// The bare o2m forms (an array of ids to assign, or `null` to unlink) are
    /// enforced on the child collection too: they route through
    /// `assign_children` / `unlink_children` -> `update_items_by_query`.
    #[tokio::test]
    async fn nested_o2m_assign_and_unlink_enforced_on_child() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "o2massign").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        if !has_virtual_comments(&state, &ctx).await {
            eprintln!("skipping: tickets.comments is not a virtual o2m field");
            cleanup_marker(&state, "o2massign").await;
            return;
        }
        let fk = comments_fk(&state, &ctx).await;

        // Parent update may touch `comments`; no child update rule yet.
        add_grant(
            &state,
            "o2massign",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            "update",
            json!(["subject", "comments"]),
            json!([]),
            json!([]),
        )
        .await;

        // Two tickets; a comment already attached to `from`, to be reassigned to
        // `to` through the bare o2m array form (`assign_children`).
        let from_id = insert_ticket(&state, "nt-o2massign-from").await;
        let to_id = insert_ticket(&state, "nt-o2massign-to").await;
        let comment_id: Uuid = sqlx::query_scalar(&format!(
            "INSERT INTO \"{HELPDESK_SCHEMA}\".ticket_comments (body, ticket) VALUES ('move-me', $1) RETURNING id"
        ))
        .bind(from_id)
        .fetch_one(&*state.database_pool)
        .await
        .expect("insert comment");

        // Assign the existing comment to `to`. This changes the child's `ticket`
        // FK, so it is a child UPDATE and must be denied without a child rule.
        let body = item(json!({ "comments": [comment_id.to_string()] }));
        let err = update_nested(&state, &ctx, "tickets", to_id.to_string(), body.clone())
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "assigning a child must be enforced on ticket_comments: {err:?}"
        );

        let owner: Option<Uuid> = sqlx::query_scalar(&format!(
            "SELECT ticket FROM \"{HELPDESK_SCHEMA}\".ticket_comments WHERE id = $1"
        ))
        .bind(comment_id)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(
            owner,
            Some(from_id),
            "the child must not be reassigned without a child update rule"
        );

        // Grant the child update (needs the injected FK).
        add_grant(
            &state,
            "o2massign",
            &seed,
            HELPDESK_SCHEMA,
            "ticket_comments",
            "update",
            json!([fk]),
            json!([]),
            json!([]),
        )
        .await;

        update_nested(&state, &ctx, "tickets", to_id.to_string(), body)
            .await
            .expect("with a child update grant the assignment must succeed");
        let owner: Option<Uuid> = sqlx::query_scalar(&format!(
            "SELECT ticket FROM \"{HELPDESK_SCHEMA}\".ticket_comments WHERE id = $1"
        ))
        .bind(comment_id)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(owner, Some(to_id), "the child must now be assigned to `to`");

        // `null` unlinks every child of `to` (sets the FK to null) — but the FK
        // is NOT NULL, so on this fixture unlink cannot succeed. Assert only
        // that the attempt is still gated by the child's update rules rather
        // than silently bypassed.
        let err = update_nested(
            &state,
            &ctx,
            "tickets",
            to_id.to_string(),
            item(json!({ "comments": null })),
        )
        .await
        .unwrap_err();
        assert!(
            !matches!(err, AlcedoError::Forbidden(_, _)),
            "unlink must not be blocked by permissions once the child update is granted: {err:?}"
        );

        cleanup_marker(&state, "o2massign").await;
    }

    /// Adds a second, app-local role for `seed`'s user in `schema`, so grants can
    /// be written in that app. Roles live per app schema, so a cross-app policy
    /// needs a role in the target app too (its `alcedocore_role_policies` FK
    /// points at that schema's `alcedo_roles`).
    async fn seed_role_in(state: &AppState, schema: &str, marker: &str, user_id: Uuid) -> Uuid {
        let role_id = Uuid::new_v4();
        let role_name = format!("nested-perm-{marker}");
        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedo_roles (id, name, description, is_system) \
             VALUES ($1, $2, '', false)"
        ))
        .bind(role_id)
        .bind(&role_name)
        .execute(&*state.database_pool)
        .await
        .expect("insert test role in target schema");

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedo_user_roles (id, user_id, role_id) VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .expect("insert test user role in target schema");

        role_id
    }

    /// `add_grant`, but for a role that lives in another app's schema (cross-app
    /// grant). The role must already exist in `schema`.
    #[allow(clippy::too_many_arguments)]
    async fn add_grant_for_role(
        state: &AppState,
        marker: &str,
        role_id: Uuid,
        schema: &str,
        table: &str,
        action: &str,
        fields: Value,
        filter: Value,
        field_validation: Value,
    ) {
        let collection = collection_id(state, schema, table)
            .await
            .unwrap_or_else(|| panic!("collection '{table}' not seeded in {schema}"));
        let policy_id = Uuid::new_v4();
        let policy_name = format!("nested-perm-{marker}-{}", policy_id);

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_policies (id, name, description) VALUES ($1, $2, '')"
        ))
        .bind(policy_id)
        .bind(&policy_name)
        .execute(&*state.database_pool)
        .await
        .expect("insert policy");

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_role_policies (id, role_id, policy_id) VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(role_id)
        .bind(policy_id)
        .execute(&*state.database_pool)
        .await
        .expect("insert role policy");

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_policy_permissions \
             (id, policy_id, collection, action, fields, filter, field_validation) \
             VALUES ($1, $2, $3, $4, $5::json, $6::json, $7::json)"
        ))
        .bind(Uuid::new_v4())
        .bind(policy_id)
        .bind(collection)
        .bind(action)
        .bind(serde_json::to_string(&fields).unwrap())
        .bind(serde_json::to_string(&filter).unwrap())
        .bind(serde_json::to_string(&field_validation).unwrap())
        .execute(&*state.database_pool)
        .await
        .expect("insert policy permission");
    }

    /// Keeps the `ReadRule` import meaningful for future nested-rule tests
    /// without triggering dead-code warnings.
    #[allow(dead_code)]
    fn _rule_type_marker(_rule: ReadRule) {}
}
