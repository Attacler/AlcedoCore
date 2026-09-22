use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get},
};
use serde_json::{Map, Value};

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        errors::AlcedoError,
        items::query::Query,
        query_parse::CustomQuery,
        respond::{JSendResponse, success},
        sessions,
    },
    utils::session_cookie::read_session_cookie,
};

pub fn sessions_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_own_sessions).delete(revoke_all_own_sessions))
        .route("/{session_id}", delete(revoke_own_session))
}

#[utoipa::path(get, path = "/api/platform/sessions", tag = "Sessions",
    responses((status = OK, body = serde_json::Value))
)]
async fn list_own_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    CustomQuery(query): CustomQuery<Query>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Vec<Map<String, Value>>>>, AlcedoError> {
    let caller = auth_level.require_user()?;

    let rows = sessions::list_query(&state, caller, query).await?;
    let current_session = read_session_cookie(&headers, &state.config.session_cookie_name);
    let rows = sessions::mark_current(rows, current_session.as_deref());

    Ok(Json(success(rows)))
}

#[utoipa::path(delete, path = "/api/platform/sessions/{session_id}", tag = "Sessions",
    params(("session_id" = String, Path, description = "Session id")),
    responses((status = OK, body = JSendResponse<bool>))
)]
async fn revoke_own_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<bool>>, AlcedoError> {
    let caller = auth_level.require_user()?;

    if !sessions::owns(&state, &session_id, caller).await? {
        return Err(AlcedoError::NotFound("Session not found".to_string(), 0));
    }
    sessions::delete(&state, &session_id).await?;

    Ok(Json(success(true)))
}

#[utoipa::path(delete, path = "/api/platform/sessions", tag = "Sessions",
    responses((status = OK, body = JSendResponse<bool>))
)]
async fn revoke_all_own_sessions(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<bool>>, AlcedoError> {
    let caller = auth_level.require_user()?;

    sessions::delete_all(&state, caller).await?;

    Ok(Json(success(true)))
}
