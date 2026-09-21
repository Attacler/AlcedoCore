use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_sessions::Session;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        auth::{AlcedoUser, AuthService},
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::service::ItemsService,
        respond::{JSendResponse, success},
    },
    utils::extract_request_uuid::extract_request_id_from_headers,
};

pub fn auth_controller() -> Router<AppState> {
    return Router::new()
        .route("/me", get(get_me))
        .route("/login", post(login_handler));
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user: AlcedoUser,
    pub scopes: Vec<String>,
    pub is_admin: bool,
}

async fn get_me(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<MeResponse>>, AlcedoError> {
    println!("{:?}", auth_level);
    let uuid = match auth_level {
        AuthLevel::User(uuid) => uuid,
        AuthLevel::Public => {
            return Err(AlcedoError::UnAuthenticated());
        }
    };

    let app_context = AppContext {
        app_name: "alcedo".to_string(),
        version: "".to_string(),
        request_source: RequestSource::API,
    };

    let collection = "alcedo_users".to_string();
    let service = ItemsService::new(&state, &app_context, &collection);

    let user = match service
        .get_items_by_pks(vec![uuid.to_string().into()])
        .await
    {
        Err(e) => {
            return Err(e);
        }
        Ok(user) => {
            if user.len() != 1 {
                return Err(AlcedoError::UnAuthenticated());
            } else {
                user.get(0).unwrap().clone()
            }
        }
    };

    let user: AlcedoUser = Value::Object(user.clone()).try_into()?;
    let response = MeResponse {
        is_admin: user.is_admin,
        scopes: vec!["all".to_string()],
        user,
    };
    Ok(Json(success(response)))
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: AlcedoUser,
}

pub async fn login_handler(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<JSendResponse<LoginResponse>>, AlcedoError> {
    let app_context = AppContext {
        app_name: "alcedo".to_string(),
        version: "".to_string(),
        request_source: RequestSource::API,
    };
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

    // Rotate session to prevent fixation
    let _ = session.delete().await;

    session
        .insert("auth_level", AuthLevel::User(user.id))
        .await
        .map_err(|e| AlcedoError::SystemError(format!("Session error: {}", e), 0))?;

    auth_service.update_last_login(user.id).await?;
    // TODO send event of login success + log

    Ok(Json(success(LoginResponse { user })))
}
