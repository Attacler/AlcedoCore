use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get},
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        query_parse::CustomQuery,
        respond::{JSendResponse, success},
        sessions,
    },
    utils::session_cookie::read_session_cookie,
};

const USERS_COLLECTION: &str = "alcedo_users";

pub fn users_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users))
        .route("/{id}", get(get_user))
        .route(
            "/{id}/sessions",
            get(get_user_sessions).delete(revoke_all_user_sessions),
        )
        .route("/{id}/sessions/{session_id}", delete(revoke_user_session))
}

fn without_secret(mut user: Map<String, Value>) -> Map<String, Value> {
    user.remove("password_hash");
    user
}

async fn caller_is_admin(state: &AppState, user_id: Uuid) -> Result<bool, AlcedoError> {
    let context = AppContext::system(RequestSource::API);
    let collection = USERS_COLLECTION.to_string();
    let service = ItemsService::new(state, &context, &collection);

    let rows = service
        .get_items_by_pks(vec![Value::String(user_id.to_string())])
        .await?;

    Ok(rows
        .first()
        .and_then(|row| row.get("is_admin"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false))
}

async fn authorize(state: &AppState, caller: Uuid, target: Uuid) -> Result<(), AlcedoError> {
    if caller == target {
        return Ok(());
    }
    if caller_is_admin(state, caller).await? {
        return Ok(());
    }
    Err(AlcedoError::Forbidden(
        "Admin access required".to_string(),
        0,
    ))
}

fn parse_user_id(id: &str) -> Result<Uuid, AlcedoError> {
    Uuid::parse_str(id).map_err(|_| AlcedoError::InvalidInput("Invalid user id".to_string(), 0))
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
    if !caller_is_admin(&state, caller).await? {
        return Err(AlcedoError::Forbidden(
            "Admin access required".to_string(),
            0,
        ));
    }

    let context = AppContext::system(RequestSource::API);
    let collection = USERS_COLLECTION.to_string();
    let service = ItemsService::new(&state, &context, &collection);

    let users = service
        .read_items_by_query(query)
        .await?
        .into_iter()
        .map(without_secret)
        .collect();

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

    let context = AppContext::system(RequestSource::API);
    let collection = USERS_COLLECTION.to_string();
    let service = ItemsService::new(&state, &context, &collection);

    let user = service
        .get_items_by_pks(vec![Value::String(target.to_string())])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AlcedoError::NotFound("User not found".to_string(), 0))?;

    Ok(Json(success(without_secret(user))))
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
