use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use alcedo_common::context::{slugify, AppContext, RequestSource};
use alcedo_common::RequestIdentity;
use alcedo_db::db::collection_items::CreateItemsBody;
use alcedo_db::db::filter_condition::{ComparisonOperator, FilterCondition, SortField};
use alcedo_db::services::items::read::{ListRequest, OneRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use alcedo_db::services::items::write::{
    execute_create_one_for_table_tx, execute_delete_for_table_by_filter_tx,
    execute_delete_for_table_tx, execute_insert_for_table_with_conflict_tx,
    execute_update_one_for_table_tx, ConflictPolicy,
};

use crate::api::permission_check::require_admin;
use crate::api::responses::ResponseEnvelope;
use crate::error::AppError;
use crate::plugins::health::AppState;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppRow {
    pub id: i32,
    pub name: String,
    pub api_name: String,
    pub icon: Option<String>,
    pub logo: Option<String>,
}

const APP_COLUMNS: &str = "id, name, api_name, icon, logo";

fn app_fields() -> Vec<String> {
    APP_COLUMNS.split(", ").map(String::from).collect()
}

#[derive(Debug, Serialize)]
pub struct AppWithVersions {
    pub id: i32,
    pub name: String,
    pub api_name: String,
    pub icon: Option<String>,
    pub logo: Option<String>,
    pub versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAppRequest {
    pub name: String,
    pub api_name: String,
    pub icon: Option<String>,
    pub logo: Option<String>,
    /// Version name to attach the new app to (e.g. "production").
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAppRequest {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub logo: Option<String>,
    /// Version names this app should be attached to (replaces the set).
    pub versions: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct VersionRow {
    pub id: i32,
    pub version_name: String,
}

impl VersionRow {
    fn from_json(value: serde_json::Value) -> Result<Self, AppError> {
        serde_json::from_value(value)
            .map_err(|e| AppError::Internal(format!("Failed to decode alcedo_versions row: {}", e)))
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateVersionRequest {
    pub version_name: String,
}

async fn refresh_schema_cache(state: &Arc<AppState>) {
    let schema = alcedo_db::services::inspector::DatabaseSchema::new()
        .refresh(&state.core)
        .await;
    *state.core.schema.write().await = schema.into();
}

/// Seed the per-app roles: `admin` (full app-management scopes) and `public` (no scopes).
async fn seed_app_roles(state: &Arc<AppState>, ctx: &AppContext) -> Result<(), AppError> {
    let pool = state.db_for(ctx).await?;
    let s = alcedo_db::db::quote_identifier(&ctx.schema_name());
    sqlx::query(&format!(
        r#"INSERT INTO {s}.alcedocore_roles (name, description, is_system)
           VALUES ('admin', 'App administrator', true), ('public', 'Default scopes for unauthenticated requests and role fallback', true)
           ON CONFLICT (name) DO NOTHING"#
    ))
    .execute(&pool)
    .await?;

    // Same scope set the bootstrap admin role receives.
    let admin_scopes = [
        "users.all",
        "roles.all",
        "plugins.all",
        "collections.all",
        "settings.read.all",
        "settings.write.all",
        "kv.all",
        "policies.all",
    ];
    for scope in admin_scopes {
        sqlx::query(&format!(
            r#"INSERT INTO {s}.alcedocore_role_scopes (role_id, scope)
               SELECT id, $1 FROM {s}.alcedocore_roles WHERE name = 'admin'
               ON CONFLICT (role_id, scope) DO NOTHING"#
        ))
        .bind(scope)
        .execute(&pool)
        .await?;
    }
    Ok(())
}

async fn ensure_app_ready(
    state: &Arc<AppState>,
    api_name: &str,
    version: &str,
) -> Result<AppContext, AppError> {
    let ctx = AppContext {
        app_name: api_name.to_string(),
        version: version.to_string(),
        request_source: RequestSource::API,
    };
    alcedo_db::app_migrations::ensure_app_version_schema(state.db()?, &ctx).await?;
    seed_app_roles(state, &ctx).await?;
    Ok(ctx)
}

/// Resolve a version name to its `alcedo_versions.id`, or `NotFound`.
async fn lookup_version_id(pool: &sqlx::PgPool, version_name: &str) -> Result<i32, AppError> {
    let version_id: Option<(i32,)> =
        sqlx::query_as(r#"SELECT id FROM alcedo.alcedo_versions WHERE version_name = $1"#)
            .bind(version_name)
            .fetch_optional(pool)
            .await?;
    version_id
        .map(|r| r.0)
        .ok_or_else(|| AppError::NotFound(format!("Version not found: {}", version_name)))
}

// Phase-4 join exception (alcedo.alcedo_versions JOIN alcedo.alcedo_apps_versions; stays bespoke)
async fn list_versions_for_app(pool: &sqlx::PgPool, app_id: i32) -> Result<Vec<String>, AppError> {
    Ok(sqlx::query_scalar(
        r#"SELECT v.version_name
           FROM alcedo.alcedo_versions v
           JOIN alcedo.alcedo_apps_versions av ON av.version_id = v.id
           WHERE av.app_id = $1
           ORDER BY v.version_name ASC"#,
    )
    .bind(app_id)
    .fetch_all(pool)
    .await?)
}

async fn ensure_unique_api_name(pool: &sqlx::PgPool, api_name: &str) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM alcedo.alcedo_apps WHERE api_name = $1)"#,
    )
    .bind(api_name)
    .fetch_one(pool)
    .await?;
    if exists {
        return Err(AppError::Conflict(format!(
            "App with api_name '{}' already exists",
            api_name
        )));
    }
    Ok(())
}

pub async fn list_apps_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
) -> Result<Json<ResponseEnvelope<Vec<AppWithVersions>>>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let collection = "alcedo_apps".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_apps".to_string(),
            },
            ListRequest {
                fields: app_fields(),
                sort: vec![SortField {
                    field: "name".to_string(),
                    order: "asc".to_string(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let apps: Vec<AppRow> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid app row: {}", e)))
        })
        .collect::<Result<_, _>>()?;
    let mut result: Vec<AppWithVersions> = Vec::new();
    for app in &apps {
        let versions = list_versions_for_app(pool, app.id).await?;
        result.push(AppWithVersions {
            id: app.id,
            name: app.name.clone(),
            api_name: app.api_name.clone(),
            icon: app.icon.clone(),
            logo: app.logo.clone(),
            versions,
        });
    }
    Ok(Json(ResponseEnvelope::success(result)))
}

pub async fn get_app_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<i32>,
) -> Result<Json<ResponseEnvelope<AppWithVersions>>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let collection = "alcedo_apps".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let app: AppRow = engine
        .read_one_for_table(
            pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_apps".to_string(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: app_fields(),
                ..Default::default()
            },
        )
        .await?
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid app row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::NotFound(format!("App not found: {}", id)))?;
    let versions = list_versions_for_app(pool, app.id).await?;
    Ok(Json(ResponseEnvelope::success(AppWithVersions {
        id: app.id,
        name: app.name.clone(),
        api_name: app.api_name.clone(),
        icon: app.icon.clone(),
        logo: app.logo.clone(),
        versions,
    })))
}

pub async fn create_app_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Json(payload): Json<CreateAppRequest>,
) -> Result<Json<ResponseEnvelope<AppWithVersions>>, AppError> {
    require_admin(&state, &identity, &headers).await?;

    let name = payload.name.trim();
    let api_name = slugify(payload.api_name.trim());
    let version = slugify(payload.version.trim());
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".to_string()));
    }
    if api_name.is_empty() {
        return Err(AppError::BadRequest("api_name is required".to_string()));
    }
    if version.is_empty() {
        return Err(AppError::BadRequest("version is required".to_string()));
    }

    let pool = state.db()?;
    ensure_unique_api_name(pool, &api_name).await?;

    // Resolve the version before creating any app/schema state.
    let version_id = lookup_version_id(pool, &version).await?;

    ensure_app_ready(&state, &api_name, &version).await?;

    // Insert the app row and its version join atomically.
    let collection = "alcedo_apps".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let app_shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let av_collection = "alcedo_apps_versions".to_string();
    let av_engine = ItemsService::for_global(&state.core, &av_collection);
    let av_shape = av_engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: av_collection.clone(),
            },
            &[],
        )
        .await?;

    let mut tx = pool.begin().await?;
    let mut app_map = serde_json::Map::new();
    app_map.insert("name".into(), serde_json::json!(name));
    app_map.insert("api_name".into(), serde_json::json!(api_name));
    if let Some(icon) = &payload.icon {
        app_map.insert("icon".into(), serde_json::json!(icon));
    }
    if let Some(logo) = &payload.logo {
        app_map.insert("logo".into(), serde_json::json!(logo));
    }
    let app_row = execute_create_one_for_table_tx(&mut tx, &app_shape, app_map).await?;
    let app: AppRow = serde_json::from_value(app_row)
        .map_err(|e| AppError::Internal(format!("Invalid app row: {}", e)))?;
    let mut av_map = serde_json::Map::new();
    av_map.insert("app_id".into(), serde_json::json!(app.id));
    av_map.insert("version_id".into(), serde_json::json!(version_id));
    execute_insert_for_table_with_conflict_tx(
        &mut tx,
        &av_shape,
        &["app_id", "version_id"],
        ConflictPolicy::DoNothing,
        av_map,
    )
    .await?;
    tx.commit().await?;

    refresh_schema_cache(&state).await;

    let versions = list_versions_for_app(pool, app.id).await?;
    Ok(Json(ResponseEnvelope::success(AppWithVersions {
        id: app.id,
        name: app.name.clone(),
        api_name: app.api_name.clone(),
        icon: app.icon.clone(),
        logo: app.logo.clone(),
        versions,
    })))
}

pub async fn update_app_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateAppRequest>,
) -> Result<Json<ResponseEnvelope<AppWithVersions>>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let app: Option<AppRow> = sqlx::query_as::<_, AppRow>(
        r#"SELECT id, name, api_name, icon, logo FROM alcedo.alcedo_apps WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let app = app.ok_or_else(|| AppError::NotFound(format!("App not found: {}", id)))?;

    let name = match &payload.name {
        Some(n) => {
            let trimmed = n.trim();
            if trimmed.is_empty() {
                return Err(AppError::BadRequest("name must not be empty".to_string()));
            }
            Some(trimmed.to_string())
        }
        None => None,
    };

    // Validate and normalize every target version before mutating anything.
    let resolved_versions: Option<Vec<(String, i32)>> = match &payload.versions {
        Some(versions) => {
            let mut resolved = Vec::with_capacity(versions.len());
            for v in versions {
                let normalized = slugify(v.trim());
                if normalized.is_empty() {
                    return Err(AppError::BadRequest(
                        "version must not be empty".to_string(),
                    ));
                }
                let version_id = lookup_version_id(pool, &normalized).await?;
                resolved.push((normalized, version_id));
            }
            Some(resolved)
        }
        None => None,
    };

    // Idempotent schema + role seeding for each target version.
    if let Some(resolved) = &resolved_versions {
        for (version, _) in resolved {
            ensure_app_ready(&state, &app.api_name, version).await?;
        }
    }

    // Replace the app row and its version joins atomically.
    let collection = "alcedo_apps".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let app_shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let av_collection = "alcedo_apps_versions".to_string();
    let av_engine = ItemsService::for_global(&state.core, &av_collection);
    let av_shape = av_engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: av_collection.clone(),
            },
            &[],
        )
        .await?;

    let mut tx = pool.begin().await?;
    let mut app_map = serde_json::Map::new();
    if let Some(name) = &name {
        app_map.insert("name".into(), serde_json::json!(name));
    }
    if let Some(icon) = &payload.icon {
        app_map.insert("icon".into(), serde_json::json!(icon));
    }
    if let Some(logo) = &payload.logo {
        app_map.insert("logo".into(), serde_json::json!(logo));
    }
    match execute_update_one_for_table_tx(&mut tx, &app_shape, &serde_json::json!(id), &app_map).await
    {
        Ok(_) => {}
        Err(AppError::NotFound(_)) => {
            return Err(AppError::NotFound(format!("App not found: {}", id)));
        }
        Err(e) => return Err(e),
    }

    if let Some(resolved) = &resolved_versions {
        execute_delete_for_table_by_filter_tx(
            &mut tx,
            &av_shape,
            FilterCondition::Rule {
                field: "app_id".into(),
                operator: ComparisonOperator::Eq,
                value: Some(serde_json::json!(id)),
            },
        )
        .await?;
        for (_, version_id) in resolved {
            let mut av_map = serde_json::Map::new();
            av_map.insert("app_id".into(), serde_json::json!(id));
            av_map.insert("version_id".into(), serde_json::json!(version_id));
            execute_insert_for_table_with_conflict_tx(
                &mut tx,
                &av_shape,
                &["app_id", "version_id"],
                ConflictPolicy::DoNothing,
                av_map,
            )
            .await?;
        }
    }
    tx.commit().await?;

    if resolved_versions.is_some() {
        refresh_schema_cache(&state).await;
    }

    let versions = list_versions_for_app(pool, id).await?;
    Ok(Json(ResponseEnvelope::success(AppWithVersions {
        id,
        name: name.unwrap_or(app.name),
        api_name: app.api_name,
        icon: payload.icon.clone().or(app.icon),
        logo: payload.logo.clone().or(app.logo),
        versions,
    })))
}

pub async fn delete_app_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let collection = "alcedo_apps".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let app_shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let av_collection = "alcedo_apps_versions".to_string();
    let av_engine = ItemsService::for_global(&state.core, &av_collection);
    let av_shape = av_engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: av_collection.clone(),
            },
            &[],
        )
        .await?;

    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    // Plugins reference `alcedo_apps_versions.id` with ON DELETE CASCADE, so
    // deleting the app-versions links first cascades plugin installs.
    execute_delete_for_table_by_filter_tx(
        &mut tx,
        &av_shape,
        FilterCondition::Rule {
            field: "app_id".into(),
            operator: ComparisonOperator::Eq,
            value: Some(serde_json::json!(id)),
        },
    )
    .await?;
    let deleted =
        execute_delete_for_table_tx(&mut tx, &app_shape, &[serde_json::json!(id)]).await?;
    if deleted.is_empty() {
        return Err(AppError::NotFound(format!("App not found: {}", id)));
    }
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    refresh_schema_cache(&state).await;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn list_versions_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
) -> Result<Json<ResponseEnvelope<Vec<VersionRow>>>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let collection = "alcedo_versions".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_versions".to_string(),
            },
            ListRequest {
                fields: vec!["id".to_string(), "version_name".to_string()],
                sort: vec![SortField {
                    field: "id".to_string(),
                    order: "asc".to_string(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let versions = result
        .items
        .into_iter()
        .map(VersionRow::from_json)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ResponseEnvelope::success(versions)))
}

pub async fn create_version_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Json(payload): Json<CreateVersionRequest>,
) -> Result<Json<ResponseEnvelope<VersionRow>>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let version_name = slugify(payload.version_name.trim());
    if version_name.is_empty() {
        return Err(AppError::BadRequest("version_name is required".to_string()));
    }
    let pool = state.db()?;
    let collection = "alcedo_versions".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let mut map = serde_json::Map::new();
    map.insert(
        "version_name".to_string(),
        serde_json::Value::String(version_name),
    );
    let outcome = engine.create(pool, CreateItemsBody::Single(map)).await?;
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("Version insert returned no row".to_string()))?;
    Ok(Json(ResponseEnvelope::success(VersionRow::from_json(row)?)))
}

pub async fn delete_version_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let collection = "alcedo_versions".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let versions_shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let dev_keys_shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: "alcedo_developer_api_keys".to_string(),
            },
            &[],
        )
        .await?;
    let av_shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: "alcedo_apps_versions".to_string(),
            },
            &[],
        )
        .await?;

    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    // Dev keys reference version_id with no ON DELETE CASCADE -> delete them first.
    execute_delete_for_table_by_filter_tx(
        &mut tx,
        &dev_keys_shape,
        FilterCondition::Rule {
            field: "version_id".into(),
            operator: ComparisonOperator::Eq,
            value: Some(serde_json::json!(id)),
        },
    )
    .await?;
    execute_delete_for_table_by_filter_tx(
        &mut tx,
        &av_shape,
        FilterCondition::Rule {
            field: "version_id".into(),
            operator: ComparisonOperator::Eq,
            value: Some(serde_json::json!(id)),
        },
    )
    .await?;
    let deleted =
        execute_delete_for_table_tx(&mut tx, &versions_shape, &[serde_json::json!(id)]).await?;
    if deleted.is_empty() {
        return Err(AppError::NotFound(format!("Version not found: {}", id)));
    }
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    refresh_schema_cache(&state).await;
    Ok(Json(serde_json::json!({ "success": true })))
}

/// GET /api/versions/:id/keys — developer API keys scoped to a version
/// (`:id` is `alcedo_versions.id`).
/// Admin-gated via `settings.read.all` (dev keys bypass scope checks).
pub async fn list_version_keys_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<Vec<crate::api::settings::DeveloperKeyResponse>>, AppError> {
    crate::api::settings::list_version_keys_impl(&state, &headers, id).await
}

#[derive(Debug, Serialize)]
pub struct UserAppAccess {
    pub app_id: i32,
    pub app_name: String,
    pub api_name: String,
    pub version: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AccessAppRef {
    pub id: i32,
    pub name: String,
    pub api_name: String,
}

#[derive(Debug, Serialize)]
pub struct AccessVersionRef {
    pub id: i32,
    pub version_name: String,
}

#[derive(Debug, Serialize)]
pub struct AccessRole {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AccessUser {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub role_ids: Vec<Uuid>,
    pub role_names: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AppVersionAccess {
    pub app_version_id: i32,
    pub app: AccessAppRef,
    pub version: AccessVersionRef,
    pub roles: Vec<AccessRole>,
    pub users: Vec<AccessUser>,
}

struct AppVersionRef {
    app_version_id: i32,
    app_id: i32,
    app_name: String,
    api_name: String,
    version_id: i32,
    version_name: String,
}

/// Resolve (app_id, version_id) → its join row + names, or NotFound.
async fn resolve_app_version_ref(
    pool: &sqlx::PgPool,
    app_id: i32,
    version_id: i32,
) -> Result<AppVersionRef, AppError> {
    let row: Option<(i32, i32, String, String, i32, String)> = sqlx::query_as(
        r#"SELECT av.id, a.id, a.name, a.api_name, v.id, v.version_name
           FROM alcedo.alcedo_apps a
           JOIN alcedo.alcedo_apps_versions av ON av.app_id = a.id
           JOIN alcedo.alcedo_versions v ON v.id = av.version_id
           WHERE a.id = $1 AND v.id = $2"#,
    )
    .bind(app_id)
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    row.map(
        |(app_version_id, app_id, app_name, api_name, version_id, version_name)| AppVersionRef {
            app_version_id,
            app_id,
            app_name,
            api_name,
            version_id,
            version_name,
        },
    )
    .ok_or_else(|| AppError::NotFound("App/version not found".to_string()))
}

// Phase-4 join exception (complex cross-schema join; stays bespoke)
pub async fn get_app_version_access_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path((app_id, version_id)): Path<(i32, i32)>,
) -> Result<Json<ResponseEnvelope<AppVersionAccess>>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let r = resolve_app_version_ref(pool, app_id, version_id).await?;

    let ctx = AppContext {
        app_name: r.api_name.clone(),
        version: r.version_name.clone(),
        request_source: RequestSource::API,
    };
    let schema_pool = state.db_for(&ctx).await?;
    let s = alcedo_db::db::quote_identifier(&ctx.schema_name());

    let roles: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(&format!(
        r#"SELECT id, name, description FROM {s}.alcedocore_roles ORDER BY name ASC"#,
    ))
    .fetch_all(&schema_pool)
    .await?;

    let rows: Vec<(Uuid, String, Option<String>, Uuid, String)> = sqlx::query_as(&format!(
        r#"SELECT u.id, u.email, u.display_name, r.id, r.name
           FROM {s}.alcedocore_user_roles ur
           JOIN {s}.alcedocore_roles r ON r.id = ur.role_id
           JOIN alcedo.alcedo_users u ON u.id = ur.user_id
           ORDER BY u.id ASC, r.name ASC"#,
    ))
    .fetch_all(&schema_pool)
    .await?;

    let mut users: Vec<AccessUser> = Vec::new();
    for (user_id, email, display_name, role_id, role_name) in rows {
        if let Some(last) = users.last_mut() {
            if last.user_id == user_id {
                last.role_ids.push(role_id);
                last.role_names.push(role_name);
                continue;
            }
        }
        users.push(AccessUser {
            user_id,
            email,
            display_name,
            role_ids: vec![role_id],
            role_names: vec![role_name],
        });
    }

    Ok(Json(ResponseEnvelope::success(AppVersionAccess {
        app_version_id: r.app_version_id,
        app: AccessAppRef {
            id: r.app_id,
            name: r.app_name,
            api_name: r.api_name,
        },
        version: AccessVersionRef {
            id: r.version_id,
            version_name: r.version_name,
        },
        roles: roles
            .into_iter()
            .map(|(id, name, description)| AccessRole { id, name, description })
            .collect(),
        users,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SetAppVersionAccessRequest {
    pub user_id: Uuid,
    pub role_ids: Vec<Uuid>,
}

pub async fn set_app_version_access_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path((app_id, version_id)): Path<(i32, i32)>,
    Json(payload): Json<SetAppVersionAccessRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let r = resolve_app_version_ref(pool, app_id, version_id).await?;

    let user_exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM alcedo.alcedo_users WHERE id = $1)"#,
    )
    .bind(payload.user_id)
    .fetch_one(pool)
    .await?;
    if !user_exists {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    let ctx = AppContext {
        app_name: r.api_name.clone(),
        version: r.version_name.clone(),
        request_source: RequestSource::API,
    };
    let schema_pool = state.db_for(&ctx).await?;
    let s = alcedo_db::db::quote_identifier(&ctx.schema_name());

    let mut role_ids = payload.role_ids.clone();
    role_ids.sort();
    role_ids.dedup();

    if !role_ids.is_empty() {
        let count: i64 = sqlx::query_scalar(&format!(
            r#"SELECT COUNT(*) FROM {s}.alcedocore_roles WHERE id = ANY($1::uuid[])"#,
        ))
        .bind(&role_ids)
        .fetch_one(&schema_pool)
        .await?;
        if count != role_ids.len() as i64 {
            return Err(AppError::BadRequest(
                "One or more role ids do not exist in the target app schema".to_string(),
            ));
        }
    }

    let mut tx = schema_pool.begin().await?;
    sqlx::query(&format!(
        r#"DELETE FROM {s}.alcedocore_user_roles WHERE user_id = $1"#,
    ))
    .bind(payload.user_id)
    .execute(&mut *tx)
    .await?;
    for role_id in &role_ids {
        sqlx::query(&format!(
            r#"INSERT INTO {s}.alcedocore_user_roles (user_id, role_id) VALUES ($1, $2)
               ON CONFLICT DO NOTHING"#,
        ))
        .bind(payload.user_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn revoke_app_version_access_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path((app_id, version_id, user_id)): Path<(i32, i32, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let pool = state.db()?;
    let r = resolve_app_version_ref(pool, app_id, version_id).await?;
    let ctx = AppContext {
        app_name: r.api_name.clone(),
        version: r.version_name.clone(),
        request_source: RequestSource::API,
    };
    let schema_pool = state.db_for(&ctx).await?;
    let s = alcedo_db::db::quote_identifier(&ctx.schema_name());
    sqlx::query(&format!(
        r#"DELETE FROM {s}.alcedocore_user_roles WHERE user_id = $1"#,
    ))
    .bind(user_id)
    .execute(&schema_pool)
    .await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn roles_for_user_in_schema(
    state: &Arc<AppState>,
    ctx: &AppContext,
    user_id: Uuid,
) -> Result<Vec<String>, AppError> {
    let pool = state.db_for(ctx).await?;
    let roles: Vec<String> = sqlx::query_scalar(
        r#"SELECT r.name FROM alcedocore_roles r
           JOIN alcedocore_user_roles ur ON ur.role_id = r.id
           WHERE ur.user_id = $1
           ORDER BY r.name ASC"#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;
    Ok(roles)
}

/// Build the app×version access list for a user. When `include_all` is set the
/// caller is an admin listing another user's access, so entries with no roles
/// are included too.
// Phase-4 join exception (complex cross-schema join; stays bespoke)
async fn collect_app_access(
    state: &Arc<AppState>,
    user_id: Uuid,
    include_all: bool,
) -> Result<Vec<UserAppAccess>, AppError> {
    let pool = state.db()?;
    let is_admin_flag = crate::api::permission_check::is_global_admin(state, user_id).await?;

    let rows: Vec<(i32, String, String, String)> = sqlx::query_as(
        r#"SELECT a.id, a.name, a.api_name, v.version_name
           FROM alcedo.alcedo_apps a
           JOIN alcedo.alcedo_apps_versions av ON av.app_id = a.id
           JOIN alcedo.alcedo_versions v ON v.id = av.version_id
           ORDER BY a.name ASC, v.version_name ASC"#,
    )
    .fetch_all(pool)
    .await?;

    let mut result: Vec<UserAppAccess> = Vec::new();
    for (app_id, app_name, api_name, version) in rows {
        let ctx = AppContext {
            app_name: api_name.clone(),
            version: version.clone(),
            request_source: RequestSource::API,
        };
        let roles = roles_for_user_in_schema(state, &ctx, user_id).await?;
        if include_all || is_admin_flag || !roles.is_empty() {
            result.push(UserAppAccess {
                app_id,
                app_name,
                api_name,
                version,
                roles,
            });
        }
    }
    Ok(result)
}

// Phase-4 join exception (complex cross-schema join; stays bespoke)
pub async fn me_apps_handler(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    identity: RequestIdentity,
) -> Result<Json<ResponseEnvelope<Vec<UserAppAccess>>>, AppError> {
    let user_id = identity
        .user_id
        .ok_or_else(|| AppError::Unauthorized("Authentication required".to_string()))?;
    let result = collect_app_access(&state, user_id, false).await?;
    Ok(Json(ResponseEnvelope::success(result)))
}

// Phase-4 join exception (complex cross-schema join; stays bespoke)
pub async fn get_user_app_access_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ResponseEnvelope<Vec<UserAppAccess>>>, AppError> {
    require_admin(&state, &identity, &headers).await?;
    let result = collect_app_access(&state, user_id, true).await?;
    Ok(Json(ResponseEnvelope::success(result)))
}

#[derive(Debug, Deserialize)]
pub struct SetUserAppAccessRequest {
    /// App api_name + version pair.
    pub app: String,
    pub version: String,
    /// Role ids to grant in that app schema. Empty = revoke all.
    pub role_ids: Vec<Uuid>,
}

pub async fn set_user_app_access_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<SetUserAppAccessRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&state, &identity, &headers).await?;

    let app = slugify(payload.app.trim());
    let version = slugify(payload.version.trim());
    if app.is_empty() {
        return Err(AppError::BadRequest("app is required".to_string()));
    }
    if version.is_empty() {
        return Err(AppError::BadRequest("version is required".to_string()));
    }

    // Validate the app×version pair before resolving (and creating any state in)
    // the target schema.
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
               SELECT 1 FROM alcedo.alcedo_apps a
               JOIN alcedo.alcedo_apps_versions av ON av.app_id = a.id
               JOIN alcedo.alcedo_versions v ON v.id = av.version_id
               WHERE a.api_name = $1 AND v.version_name = $2)"#,
    )
    .bind(&app)
    .bind(&version)
    .fetch_one(state.db()?)
    .await?;
    if !exists {
        return Err(AppError::NotFound("App/version not found".to_string()));
    }

    let ctx = AppContext {
        app_name: app,
        version,
        request_source: RequestSource::API,
    };
    let pool = state.db_for(&ctx).await?;
    let s = alcedo_db::db::quote_identifier(&ctx.schema_name());

    let mut role_ids = payload.role_ids.clone();
    role_ids.sort();
    role_ids.dedup();

    // Verify roles belong to this app schema.
    let count: i64 = sqlx::query_scalar(&format!(
        r#"SELECT COUNT(*) FROM {s}.alcedocore_roles WHERE id = ANY($1::uuid[])"#,
    ))
    .bind(&role_ids)
    .fetch_one(&pool)
    .await?;
    if count != role_ids.len() as i64 {
        return Err(AppError::BadRequest(
            "One or more role ids do not exist in the target app schema".to_string(),
        ));
    }

    let mut tx = pool.begin().await?;
    sqlx::query(&format!(
        r#"DELETE FROM {s}.alcedocore_user_roles WHERE user_id = $1"#,
    ))
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    for role_id in &role_ids {
        sqlx::query(&format!(
            r#"INSERT INTO {s}.alcedocore_user_roles (user_id, role_id) VALUES ($1, $2)
               ON CONFLICT DO NOTHING"#,
        ))
        .bind(user_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "success": true })))
}

pub fn apps_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/apps", get(list_apps_handler).post(create_app_handler))
        .route(
            "/api/apps/:id",
            get(get_app_handler)
                .put(update_app_handler)
                .delete(delete_app_handler),
        )
        .route(
            "/api/versions",
            get(list_versions_handler).post(create_version_handler),
        )
        .route("/api/versions/:id", delete(delete_version_handler))
        .route("/api/versions/:id/keys", get(list_version_keys_handler))
        .route("/api/me/apps", get(me_apps_handler))
        .route(
            "/api/users/:id/app-access",
            get(get_user_app_access_handler).put(set_user_app_access_handler),
        )
        .route(
            "/api/apps/:app_id/versions/:version_id/access",
            get(get_app_version_access_handler).put(set_app_version_access_handler),
        )
        .route(
            "/api/apps/:app_id/versions/:version_id/access/:user_id",
            delete(revoke_app_version_access_handler),
        )
        .with_state(state)
}
