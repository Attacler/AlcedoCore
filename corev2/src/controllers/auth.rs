use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        auth::{AlcedoUser, AuthService},
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::service::ItemsService,
        respond::{JSendResponse, success},
        roles::RolesService,
        sessions,
    },
    utils::session_cookie::{build_session_cookie, clear_session_cookie, read_session_cookie},
};

pub fn auth_controller() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_me))
        .route("/login", post(login_handler))
        .route("/logout", post(logout_handler))
}

fn response_with_cookie(body: impl IntoResponse, cookie: String) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("session cookie must be a valid header value"),
    );
    response
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user: AlcedoUser,
    pub scopes: Vec<String>,
    pub is_admin: bool,
}

#[utoipa::path(get, path = "/api/platform/auth/me", tag = "Auth",
    responses((status = OK, body = serde_json::Value))
)]
async fn get_me(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    headers: HeaderMap,
) -> Result<Json<JSendResponse<MeResponse>>, AlcedoError> {
    let uuid = auth_level.require_user()?;

    let app_context = AppContext::system(RequestSource::API);

    let collection = "alcedo_users".to_string();
    let service = ItemsService::new(&state, &app_context, &collection);

    let user = service
        .get_single_item_by_pk(uuid.to_string().into())
        .await?
        .ok_or_else(AlcedoError::UnAuthenticated)?;

    let user: AlcedoUser = Value::Object(user).try_into()?;

    // Scopes are app-scoped: resolve them against the request's app context.
    let mut scopes = match (
        headers.get("x-app").and_then(|v| v.to_str().ok()),
        headers.get("x-version").and_then(|v| v.to_str().ok()),
    ) {
        (Some(app), Some(version)) => {
            let ctx = AppContext {
                app_name: app.to_string(),
                version: version.to_string(),
                request_source: RequestSource::API,
            };
            RolesService::new(&state, &ctx)
                .scopes_for_user(uuid)
                .await
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    if user.is_admin {
        scopes.push("rootaccess.all".to_string());
    }

    let response = MeResponse {
        is_admin: user.is_admin,
        scopes,
        user,
    };
    Ok(Json(success(response)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: AlcedoUser,
}

#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    pub success: bool,
}

#[utoipa::path(post, path = "/api/platform/auth/login", tag = "Auth",
    request_body = LoginRequest,
    responses((status = OK, body = serde_json::Value))
)]
pub async fn login_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Response, AlcedoError> {
    let app_context = AppContext::system(RequestSource::API);
    // let request_id = extract_request_id_from_headers(&headers);

    let auth_service = AuthService::new(&state, &app_context);
    auth_service.check_login_lockout(&payload.email).await?;

    let user = match auth_service.find_user_by_email(&payload.email).await? {
        Some(u) => u,
        None => {
            auth_service.record_failed_login(&payload.email).await;
            // TODO send event of login failure + log
            return Err(AlcedoError::Unauthorized(
                "Invalid email or password".to_string(),
                0,
            ));
        }
    };

    let valid = AuthService::verify_password(&payload.password, &user.password_hash).await?;
    if !valid {
        auth_service.record_failed_login(&payload.email).await;
        // TODO send event of login failure + log
        return Err(AlcedoError::Unauthorized(
            "Invalid email or password".to_string(),
            0,
        ));
    }

    auth_service.clear_login_failures(&payload.email).await;

    // Rotate any session referenced by the incoming cookie.
    if let Some(old_session_id) = read_session_cookie(&headers, &state.config.session_cookie_name) {
        let _ = sessions::delete(&state, &old_session_id).await;
    }

    let ttl = Duration::from_secs(state.config.session_ttl_seconds.max(1) as u64);
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let session_id = sessions::create(&state, user.id, user_agent, ttl).await?;

    auth_service.update_last_login(user.id).await?;
    // TODO send event of login success + log

    let cookie = build_session_cookie(
        &session_id.to_string(),
        state.config.session_ttl_seconds,
        &state.config,
    );
    Ok(response_with_cookie(
        Json(success(LoginResponse { user })),
        cookie,
    ))
}

#[utoipa::path(post, path = "/api/platform/auth/logout", tag = "Auth",
    responses((status = OK, body = serde_json::Value))
)]
pub async fn logout_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AlcedoError> {
    if let Some(session_id) = read_session_cookie(&headers, &state.config.session_cookie_name) {
        let _ = sessions::delete(&state, &session_id).await;
    }

    Ok(response_with_cookie(
        Json(success(LogoutResponse { success: true })),
        clear_session_cookie(&state.config),
    ))
}
