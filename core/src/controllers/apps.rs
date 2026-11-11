use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    controllers::require_admin,
    item_map,
    middelware::auth::AuthLevel,
    migrations::app_migrations::run_app_migrations,
    services::{
        apps::{AppsService, version_names_for_app},
        auth::AuthService,
        collections::schema::{SchemaService, drop_schema},
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        respond::{JSendResponse, success},
        roles::RolesService,
        versions::service::VersionsService,
    },
    utils::{parse_uuid_named, slugify},
};

pub fn apps_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_apps).post(create_app))
        .route("/{id}", get(get_app).put(update_app).delete(delete_app))
        .route(
            "/{app_id}/versions/{version_id}/access",
            get(get_app_version_access).put(set_app_version_access),
        )
        .route(
            "/{app_id}/versions/{version_id}/access/{user_id}",
            axum::routing::delete(revoke_app_version_access),
        )
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AppWithVersions {
    pub id: i32,
    pub name: String,
    pub api_name: String,
    pub icon: Option<String>,
    pub logo: Option<String>,
    pub versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AppRow {
    pub id: i32,
    pub name: String,
    pub api_name: String,
    pub icon: Option<String>,
    pub logo: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateAppRequest {
    pub name: String,
    pub api_name: String,
    pub icon: Option<String>,
    pub logo: Option<String>,
    pub version: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateAppRequest {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub logo: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserAppAccess {
    pub app_id: i32,
    pub app_name: String,
    pub api_name: String,
    pub version: String,
    pub roles: Vec<String>,
}

fn app_row_from_json(value: Value) -> Result<AppRow, AlcedoError> {
    serde_json::from_value(value)
        .map_err(|e| AlcedoError::SystemError(format!("Failed to decode app row: {}", e), 0))
}

async fn list_apps_responses(state: &AppState) -> Result<Vec<AppWithVersions>, AlcedoError> {
    let rows = AppsService::new(state).list_app_version_rows().await?;

    let mut apps: Vec<AppWithVersions> = Vec::new();
    for row in &rows {
        let Some(app_value) = row.get("app_id") else {
            continue;
        };
        let app = app_row_from_json(app_value.clone())?;
        if apps.iter().any(|a| a.id == app.id) {
            continue;
        }
        apps.push(AppWithVersions {
            versions: version_names_for_app(&rows, app.id),
            id: app.id,
            name: app.name,
            api_name: app.api_name,
            icon: app.icon,
            logo: app.logo,
        });
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(apps)
}

async fn fetch_app_row(state: &AppState, id: i32) -> Result<Option<AppRow>, AlcedoError> {
    let context = AppContext::system(RequestSource::API);
    let collection = "alcedocore_apps".to_string();
    let service = ItemsService::new(state, &context, &collection);
    let row = service
        .get_single_item_by_pk(Value::String(id.to_string()))
        .await?;
    row.map(|row| app_row_from_json(Value::Object(row)))
        .transpose()
}

async fn fetch_app(state: &AppState, id: i32) -> Result<Option<AppWithVersions>, AlcedoError> {
    let Some(app) = fetch_app_row(state, id).await? else {
        return Ok(None);
    };
    let all = AppsService::new(state).list_app_version_rows().await?;
    Ok(Some(AppWithVersions {
        versions: version_names_for_app(&all, app.id),
        id: app.id,
        name: app.name,
        api_name: app.api_name,
        icon: app.icon,
        logo: app.logo,
    }))
}

#[utoipa::path(get, path = "/api/platform/apps", tag = "Apps",
    responses((status = OK, body = JSendResponse<Vec<AppWithVersions>>))
)]
async fn list_apps(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Vec<AppWithVersions>>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let apps = list_apps_responses(&state).await?;
    Ok(Json(success(apps)))
}

#[utoipa::path(get, path = "/api/platform/apps/{id}", tag = "Apps",
    params(("id" = i32, Path, description = "App id")),
    responses((status = OK, body = JSendResponse<AppWithVersions>))
)]
async fn get_app(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<AppWithVersions>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let app = fetch_app(&state, id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("App not found: {}", id), 0))?;
    Ok(Json(success(app)))
}

#[utoipa::path(post, path = "/api/platform/apps", tag = "Apps",
    request_body = CreateAppRequest,
    responses((status = OK, body = JSendResponse<AppWithVersions>))
)]
async fn create_app(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<CreateAppRequest>,
) -> Result<Json<JSendResponse<AppWithVersions>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let name = payload.name.trim().to_string();
    let api_name = slugify(&payload.api_name.trim());
    let version = slugify(&payload.version.trim());
    if name.is_empty() {
        return Err(AlcedoError::InvalidInput("name is required".to_string(), 0));
    }
    if api_name.is_empty() {
        return Err(AlcedoError::InvalidInput(
            "api_name is required".to_string(),
            0,
        ));
    }
    if version.is_empty() {
        return Err(AlcedoError::InvalidInput(
            "version is required".to_string(),
            0,
        ));
    }
    if AppsService::new(&state).api_name_exists(&api_name).await? {
        return Err(AlcedoError::InvalidInput(
            format!("App with api_name '{}' already exists", api_name),
            0,
        ));
    }

    let version_id = match VersionsService::new(&state)
        .get_version_id_by_name(&version)
        .await?
    {
        Some(id) => id,
        None => {
            return Err(AlcedoError::NotFound(
                format!("Version not found: {}", &version),
                0,
            ));
        }
    };
    let context = AppContext::system(RequestSource::API);
    let collection = "alcedocore_apps".to_string();
    let mut service = ItemsService::new(&state, &context, &collection);
    let mut app_map = item_map! {
        "name" => name,
        "api_name" => api_name.clone(),
    };
    if let Some(icon) = &payload.icon {
        app_map.insert("icon".to_string(), Value::String(icon.clone()));
    }
    if let Some(logo) = &payload.logo {
        app_map.insert("logo".to_string(), Value::String(logo.clone()));
    }
    let created = service.create_many(vec![app_map], &mut None).await?;
    let app_id = created
        .first()
        .and_then(|pk| pk.parse::<i32>().ok())
        .ok_or_else(|| AlcedoError::SystemError("App insert returned no id".to_string(), 0))?;

    AppsService::new(&state)
        .link_apps_to_version(&[(app_id, version_id)])
        .await?;
    run_app_migrations(&state.database_pool).await;
    SchemaService::refresh_schema_and_meta(&state).await;

    let app = fetch_app(&state, app_id)
        .await?
        .ok_or_else(|| AlcedoError::SystemError("App was not created".to_string(), 0))?;

    Ok(Json(success(app)))
}

#[utoipa::path(put, path = "/api/platform/apps/{id}", tag = "Apps",
    params(("id" = i32, Path, description = "App id")),
    request_body = UpdateAppRequest,
    responses((status = OK, body = JSendResponse<AppWithVersions>))
)]
async fn update_app(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateAppRequest>,
) -> Result<Json<JSendResponse<AppWithVersions>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    fetch_app(&state, id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("App not found: {}", id), 0))?;

    let mut app_map = Map::new();
    if let Some(name) = &payload.name {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AlcedoError::InvalidInput(
                "name must not be empty".to_string(),
                0,
            ));
        }
        app_map.insert("name".to_string(), Value::String(trimmed.to_string()));
    }
    if let Some(icon) = &payload.icon {
        app_map.insert("icon".to_string(), Value::String(icon.clone()));
    }
    if let Some(logo) = &payload.logo {
        app_map.insert("logo".to_string(), Value::String(logo.clone()));
    }

    if !app_map.is_empty() {
        let context = AppContext::system(RequestSource::API);
        let collection = "alcedocore_apps".to_string();
        let mut service = ItemsService::new(&state, &context, &collection);
        service
            .update_items_by_query(&mut Query::eq("id", Value::from(id)), app_map, &mut None)
            .await?;
    }

    let app = fetch_app(&state, id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("App not found: {}", id), 0))?;
    Ok(Json(success(app)))
}

#[utoipa::path(delete, path = "/api/platform/apps/{id}", tag = "Apps",
    params(("id" = i32, Path, description = "App id")),
    responses((status = OK, body = serde_json::Value))
)]
async fn delete_app(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<serde_json::Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    // Resolve the app directly (not via links) so orphaned apps stay deletable.
    let app = fetch_app_row(&state, id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("App not found: {}", id), 0))?;

    // An app is removed from every version it is attached to.
    let link_rows = AppsService::new(&state).list_app_version_rows().await?;
    let versions = version_names_for_app(&link_rows, app.id);
    for version_name in &versions {
        let schema_name = AppContext {
            app_name: app.api_name.clone(),
            version: version_name.clone(),
            request_source: RequestSource::API,
            identity: None,
            request_id: None,
        }
        .schema_name();
        drop_schema(&state, &schema_name).await?;
    }

    AppsService::new(&state).delete_app_links(id).await?;

    let context = AppContext::system(RequestSource::API);
    let collection = "alcedocore_apps".to_string();
    let mut service = ItemsService::new(&state, &context, &collection);
    let deleted = service
        .delete_items_by_pks(vec![Value::String(id.to_string())], None)
        .await?;
    if deleted == 0 {
        return Err(AlcedoError::NotFound(format!("App not found: {}", id), 0));
    }
    SchemaService::refresh_schema_and_meta(&state).await;

    Ok(Json(success(serde_json::json!({ "success": true }))))
}

/// Apps/versions the current user can access. Admins see everything.
#[utoipa::path(get, path = "/api/platform/me/apps", tag = "Apps",
    responses((status = OK, body = JSendResponse<Vec<UserAppAccess>>))
)]
pub async fn me_apps(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Vec<UserAppAccess>>>, AlcedoError> {
    let (user_id, key_version_id) = match auth_level {
        AuthLevel::User(user_id) => (Some(user_id), None),
        // Developer keys are root for the version they were issued for.
        AuthLevel::DeveloperKey { version_id } => (None, Some(version_id)),
        AuthLevel::Public | AuthLevel::Plugin(_) => {
            return Err(AlcedoError::UnAuthenticated());
        }
    };

    let is_admin = match user_id {
        Some(user_id) => {
            AuthService::new(&state, &AppContext::system(RequestSource::API))
                .is_admin(user_id)
                .await?
        }
        None => key_version_id.is_some(),
    };
    let rows = AppsService::new(&state).list_app_version_rows().await?;

    let mut result: Vec<UserAppAccess> = Vec::new();
    for row in &rows {
        let Some(app_value) = row.get("app_id") else {
            continue;
        };
        let Some(version_value) = row.get("version_id") else {
            continue;
        };
        let Some(app_id) = app_value.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let Some(api_name) = app_value.get("api_name").and_then(Value::as_str) else {
            continue;
        };
        let Some(version_name) = version_value.get("version_name").and_then(Value::as_str) else {
            continue;
        };

        if let Some(key_version_id) = key_version_id {
            if version_value.get("id").and_then(Value::as_i64) != Some(key_version_id as i64) {
                continue;
            }
        }

        let roles = match user_id {
            Some(user_id) => roles_for_user(&state, api_name, version_name, user_id).await?,
            None => Vec::new(),
        };
        if !is_admin && roles.is_empty() {
            continue;
        }

        result.push(UserAppAccess {
            app_id: app_id as i32,
            app_name: app_value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(api_name)
                .to_string(),
            api_name: api_name.to_string(),
            version: version_name.to_string(),
            roles,
        });
    }

    result.sort_by(|a, b| {
        a.app_name
            .to_lowercase()
            .cmp(&b.app_name.to_lowercase())
            .then(a.version.to_lowercase().cmp(&b.version.to_lowercase()))
    });

    Ok(Json(success(result)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetUserAppAccessRequest {
    pub app: String,
    pub version: String,
    #[serde(default)]
    pub role_ids: Vec<Uuid>,
}

/// Per user, every app×version they can access plus their role names. Admins
/// listing another user get entries with no roles included too.
#[utoipa::path(get, path = "/api/platform/users/{id}/app-access", tag = "Apps",
    responses((status = OK, body = JSendResponse<Value>))
)]
pub async fn get_user_app_access(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let user_id = parse_uuid_named(&id, "user id")?;
    let context = AppContext::system_request(RequestSource::API, &headers);
    let service = RolesService::new(&state, &context);
    let access = service.collect_user_app_access(user_id, true).await?;
    Ok(Json(success(Value::Array(access))))
}

/// Replaces a user's roles within a single app×version schema.
#[utoipa::path(put, path = "/api/platform/users/{id}/app-access", tag = "Apps",
    request_body = SetUserAppAccessRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
pub async fn set_user_app_access(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<SetUserAppAccessRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let user_id = parse_uuid_named(&id, "user id")?;
    let context = AppContext::system_request(RequestSource::API, &headers);
    let service = RolesService::new(&state, &context);
    service
        .set_user_app_access(user_id, &payload.app, &payload.version, &payload.role_ids)
        .await?;
    Ok(Json(success(serde_json::json!({ "success": true }))))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetAppVersionAccessRequest {
    pub user_id: Uuid,
    pub role_ids: Vec<Uuid>,
}

/// List all roles and users-with-roles on a specific app×version.
#[utoipa::path(get, path = "/api/platform/apps/{app_id}/versions/{version_id}/access", tag = "Apps",
    responses((status = OK, body = JSendResponse<Value>))
)]
pub async fn get_app_version_access(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((app_id, version_id)): Path<(i32, i32)>,
    headers: HeaderMap,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let (api_name, version_name, app_name) =
        AppsService::new(&state)
            .resolve_app_version_ref(app_id, version_id)
            .await?;

    let ctx = AppContext {
        app_name: api_name.clone(),
        version: version_name.clone(),
        request_source: RequestSource::API,
        identity: None,
        request_id: None,
    };

    // All roles in this app-version schema
    let roles_table = "alcedocore_roles".to_string();
    let roles_service = ItemsService::new(&state, &ctx, &roles_table);
    let mut roles_query = Query::default();
    roles_query.limit = 0;
    roles_query.fields = vec!["id".to_string(), "name".to_string(), "description".to_string()];
    let roles = roles_service.read_items_by_query(roles_query).await?;

    // All user_roles links in this schema
    let user_roles_table = "alcedocore_user_roles".to_string();
    let ur_service = ItemsService::new(&state, &ctx, &user_roles_table);
    let mut ur_query = Query::default();
    ur_query.limit = 0;
    ur_query.fields = vec!["user_id".to_string(), "role_id".to_string()];
    let user_roles = ur_service.read_items_by_query(ur_query).await?;

    // All users (system schema)
    let system_ctx = AppContext::system_request(RequestSource::API, &headers);
    let users_table = "alcedocore_users".to_string();
    let users_service = ItemsService::new(&state, &system_ctx, &users_table);
    let mut users_query = Query::default();
    users_query.limit = 0;
    users_query.fields = vec![
        "id".to_string(),
        "email".to_string(),
        "display_name".to_string(),
    ];
    let users = users_service.read_items_by_query(users_query).await?;

    // Build user → roles map
    let role_map: std::collections::HashMap<String, Value> = roles
        .iter()
        .filter_map(|r| {
            let id = r.get("id")?.as_str()?.to_string();
            Some((id, Value::Object(r.clone())))
        })
        .collect();

    let mut users_out: Vec<Value> = Vec::new();
    for user in &users {
        let Some(uid) = user.get("id").and_then(Value::as_str) else { continue };
        let role_ids: Vec<String> = user_roles
            .iter()
            .filter_map(|ur| {
                let ur_uid = ur.get("user_id")?.as_str()?;
                if ur_uid == uid {
                    ur.get("role_id")?.as_str().map(str::to_string)
                } else {
                    None
                }
            })
            .collect();
        if role_ids.is_empty() {
            continue;
        }
        let role_names: Vec<String> = role_ids
            .iter()
            .filter_map(|rid| {
                role_map
                    .get(rid)
                    .and_then(|r| r.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        users_out.push(serde_json::json!({
            "user_id": uid,
            "email": user.get("email").cloned().unwrap_or(Value::Null),
            "display_name": user.get("display_name").cloned().unwrap_or(Value::Null),
            "role_ids": role_ids,
            "role_names": role_names,
        }));
    }

    Ok(Json(success(serde_json::json!({
        "app": { "id": app_id, "name": app_name, "api_name": api_name },
        "version": { "id": version_id, "version_name": version_name },
        "roles": roles,
        "users": users_out,
    }))))
}

/// Replace a user's role set on a specific app×version.
#[utoipa::path(put, path = "/api/platform/apps/{app_id}/versions/{version_id}/access", tag = "Apps",
    request_body = SetAppVersionAccessRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
pub async fn set_app_version_access(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((app_id, version_id)): Path<(i32, i32)>,
    headers: HeaderMap,
    Json(payload): Json<SetAppVersionAccessRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let (api_name, version_name, _) =
        AppsService::new(&state)
            .resolve_app_version_ref(app_id, version_id)
            .await?;

    // Validate user exists
    let system_ctx = AppContext::system_request(RequestSource::API, &headers);
    let users_table = "alcedocore_users".to_string();
    let users_service = ItemsService::new(&state, &system_ctx, &users_table);
    let user_missing = users_service
        .read_items_by_query(Query::eq(
            "id",
            Value::String(payload.user_id.to_string()),
        ))
        .await?
        .is_empty();
    if user_missing {
        return Err(AlcedoError::NotFound("User not found".to_string(), 0));
    }

    let context = AppContext::system_request(RequestSource::API, &headers);
    let service = RolesService::new(&state, &context);
    service
        .set_user_app_access(payload.user_id, &api_name, &version_name, &payload.role_ids)
        .await?;
    Ok(Json(success(serde_json::json!({ "success": true }))))
}

/// Revoke a user's access entirely from a specific app×version.
#[utoipa::path(delete, path = "/api/platform/apps/{app_id}/versions/{version_id}/access/{user_id}", tag = "Apps",
    responses((status = OK, body = JSendResponse<Value>))
)]
pub async fn revoke_app_version_access(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((app_id, version_id, user_id)): Path<(i32, i32, String)>,
    headers: HeaderMap,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let user_id = parse_uuid_named(&user_id, "user id")?;
    let (api_name, version_name, _) =
        AppsService::new(&state)
            .resolve_app_version_ref(app_id, version_id)
            .await?;

    let ctx = AppContext {
        app_name: api_name,
        version: version_name,
        request_source: RequestSource::API,
        identity: None,
        request_id: None,
    };
    let user_roles_table = "alcedocore_user_roles".to_string();
    let mut service = ItemsService::new(&state, &ctx, &user_roles_table);
    service
        .delete_items_by_query(
            Query::eq("user_id", serde_json::json!(user_id.to_string())),
            &mut None,
        )
        .await?;
    Ok(Json(success(serde_json::json!({ "success": true }))))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AppVersionRow {
    pub id: i32,
    pub app_id: i32,
    pub app_name: String,
    pub api_name: String,
    pub version_id: i32,
    pub version_name: String,
}

#[utoipa::path(get, path = "/api/platform/app-versions", tag = "Apps",
    responses((status = OK, body = JSendResponse<Vec<AppVersionRow>>))
)]
pub async fn list_app_versions(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Vec<AppVersionRow>>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let rows = AppsService::new(&state).list_app_version_rows().await?;
    let mut result: Vec<AppVersionRow> = Vec::new();
    for row in &rows {
        let Some(id) = row.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let app = row.get("app_id");
        let version = row.get("version_id");
        result.push(AppVersionRow {
            id: id as i32,
            app_id: app
                .and_then(|a| a.get("id"))
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32,
            app_name: app
                .and_then(|a| a.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            api_name: app
                .and_then(|a| a.get("api_name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            version_id: version
                .and_then(|v| v.get("id"))
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32,
            version_name: version
                .and_then(|v| v.get("version_name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        });
    }
    result.sort_by(|a, b| {
        a.app_name
            .to_lowercase()
            .cmp(&b.app_name.to_lowercase())
            .then(a.version_name.cmp(&b.version_name))
    });
    Ok(Json(success(result)))
}

async fn roles_for_user(
    state: &AppState,
    api_name: &str,
    version_name: &str,
    user_id: uuid::Uuid,
) -> Result<Vec<String>, AlcedoError> {
    let context = AppContext {
        app_name: api_name.to_string(),
        version: version_name.to_string(),
        request_source: RequestSource::API,
        identity: None,
        request_id: None,
    };
    let collection = "alcedocore_user_roles".to_string();
    let service = ItemsService::new(state, &context, &collection);
    let rows = service
        .read_items_by_query(Query {
            fields: vec!["role_id.*".to_string()],
            limit: 0,
            ..Query::eq("user_id", Value::String(user_id.to_string()))
        })
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get("role_id")?.get("name")?.as_str().map(String::from))
        .collect())
}
