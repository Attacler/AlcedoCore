use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get},
};
use serde::Deserialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        errors::AlcedoError,
        items::query::Query,
        query_parse::CustomQuery,
        respond::{JSendResponse, success},
        sessions,
        users::UsersService,
    },
    utils::{parse_uuid_named, session_cookie::read_session_cookie},
};

pub fn users_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users).post(create_user))
        .route("/{id}", get(get_user).put(update_user).delete(delete_user))
        .route("/{id}/password", axum::routing::post(change_password))
        .route(
            "/{id}/sessions",
            get(get_user_sessions).delete(revoke_all_user_sessions),
        )
        .route("/{id}/sessions/{session_id}", delete(revoke_user_session))
}

async fn require_admin(state: &AppState, caller: Uuid) -> Result<(), AlcedoError> {
    if UsersService::new(state).is_admin(caller).await? {
        return Ok(());
    }
    Err(AlcedoError::Forbidden(
        "Admin access required".to_string(),
        0,
    ))
}

async fn authorize(state: &AppState, caller: Uuid, target: Uuid) -> Result<(), AlcedoError> {
    if caller == target {
        return Ok(());
    }
    require_admin(state, caller).await
}

fn parse_user_id(id: &str) -> Result<Uuid, AlcedoError> {
    parse_uuid_named(id, "user id")
}

#[utoipa::path(get, path = "/api/platform/users", tag = "Users",
    responses((status = OK, body = serde_json::Value))
)]
async fn list_users(
    State(state): State<AppState>,
    CustomQuery(query): CustomQuery<Query>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Vec<Map<String, Value>>>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    require_admin(&state, caller).await?;

    let users = UsersService::new(&state).list(query).await?;
    Ok(Json(success(users)))
}

#[utoipa::path(get, path = "/api/platform/users/{id}", tag = "Users",
    params(("id" = Uuid, Path, description = "User id")),
    responses((status = OK, body = serde_json::Value))
)]
async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Map<String, Value>>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    let target = parse_user_id(&id)?;
    authorize(&state, caller, target).await?;

    let user = UsersService::new(&state)
        .get(target)
        .await?
        .ok_or_else(|| AlcedoError::NotFound("User not found".to_string(), 0))?;

    Ok(Json(success(user)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
}

#[utoipa::path(post, path = "/api/platform/users", tag = "Users",
    request_body = CreateUserRequest,
    responses((status = OK, body = serde_json::Value))
)]
async fn create_user(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<JSendResponse<Map<String, Value>>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    require_admin(&state, caller).await?;

    let created = UsersService::new(&state)
        .create(
            &payload.email,
            &payload.password,
            payload.display_name,
            payload.is_admin,
        )
        .await?;

    Ok(Json(success(created)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
}

#[utoipa::path(put, path = "/api/platform/users/{id}", tag = "Users",
    params(("id" = Uuid, Path, description = "User id")),
    request_body = UpdateUserRequest,
    responses((status = OK, body = serde_json::Value))
)]
async fn update_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
    auth_level: AuthLevel,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<JSendResponse<Map<String, Value>>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    let target = parse_user_id(&id)?;
    authorize(&state, caller, target).await?;

    if payload.is_admin.is_some() && !UsersService::new(&state).is_admin(caller).await? {
        return Err(AlcedoError::Forbidden(
            "Only admins can change admin status".to_string(),
            0,
        ));
    }

    let user = UsersService::new(&state)
        .update(target, payload.email, payload.display_name, payload.is_admin)
        .await?;

    Ok(Json(success(user)))
}

#[utoipa::path(delete, path = "/api/platform/users/{id}", tag = "Users",
    params(("id" = Uuid, Path, description = "User id")),
    responses((status = OK, body = JSendResponse<bool>))
)]
async fn delete_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<bool>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    let target = parse_user_id(&id)?;
    require_admin(&state, caller).await?;
    if caller == target {
        return Err(AlcedoError::InvalidInput(
            "You cannot delete your own account".to_string(),
            0,
        ));
    }

    if !UsersService::new(&state).delete(target).await? {
        return Err(AlcedoError::NotFound("User not found".to_string(), 0));
    }

    Ok(Json(success(true)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    pub current_password: Option<String>,
    pub new_password: String,
}

#[utoipa::path(post, path = "/api/platform/users/{id}/password", tag = "Users",
    params(("id" = Uuid, Path, description = "User id")),
    request_body = ChangePasswordRequest,
    responses((status = OK, body = JSendResponse<bool>))
)]
async fn change_password(
    State(state): State<AppState>,
    Path(id): Path<String>,
    auth_level: AuthLevel,
    Json(payload): Json<ChangePasswordRequest>,
) -> Result<Json<JSendResponse<bool>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    let target = parse_user_id(&id)?;
    authorize(&state, caller, target).await?;

    // Changing your own password requires proving the current one; admins may
    // reset another user's password without it.
    if caller == target && payload.current_password.is_none() {
        return Err(AlcedoError::InvalidInput(
            "Current password is required".to_string(),
            0,
        ));
    }

    UsersService::new(&state)
        .change_password(
            target,
            payload.current_password.as_deref(),
            &payload.new_password,
        )
        .await?;

    Ok(Json(success(true)))
}

#[utoipa::path(get, path = "/api/platform/users/{id}/sessions", tag = "Users",
    params(("id" = Uuid, Path, description = "User id")),
    responses((status = OK, body = serde_json::Value))
)]
async fn get_user_sessions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CustomQuery(query): CustomQuery<Query>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Vec<Map<String, Value>>>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    let target = parse_user_id(&id)?;
    authorize(&state, caller, target).await?;

    let rows = sessions::list_query(&state, target, query).await?;
    let current_session = read_session_cookie(&headers, &state.config.session_cookie_name);
    let rows = sessions::mark_current(rows, current_session.as_deref());

    Ok(Json(success(rows)))
}

#[utoipa::path(delete, path = "/api/platform/users/{id}/sessions/{session_id}", tag = "Users",
    params(
        ("id" = Uuid, Path, description = "User id"),
        ("session_id" = String, Path, description = "Session id"),
    ),
    responses((status = OK, body = JSendResponse<bool>))
)]
async fn revoke_user_session(
    State(state): State<AppState>,
    Path((id, session_id)): Path<(String, String)>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<bool>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    let target = parse_user_id(&id)?;
    authorize(&state, caller, target).await?;

    if !sessions::owns(&state, &session_id, target).await? {
        return Err(AlcedoError::NotFound("Session not found".to_string(), 0));
    }
    sessions::delete(&state, &session_id).await?;

    Ok(Json(success(true)))
}

#[utoipa::path(delete, path = "/api/platform/users/{id}/sessions", tag = "Users",
    params(("id" = Uuid, Path, description = "User id")),
    responses((status = OK, body = JSendResponse<bool>))
)]
async fn revoke_all_user_sessions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<bool>>, AlcedoError> {
    let caller = auth_level.require_user()?;
    let target = parse_user_id(&id)?;
    authorize(&state, caller, target).await?;

    sessions::delete_all(&state, target).await?;

    Ok(Json(success(true)))
}
