use axum::{
    extract::State,
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_sessions::Session;

use crate::db::activity_logs::SystemLogEntry;
use crate::error::AppError;
use crate::middleware::logging::extract_request_id_from_headers;
use crate::plugins::health::AppState;
use crate::services::auth;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: auth::PublicUser,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user: auth::PublicUser,
    pub scopes: Vec<String>,
}

const SESSION_USER_ID_KEY: &str = "user_id";

pub fn auth_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/auth/login", post(login_handler))
        .route("/api/auth/logout", post(logout_handler))
        .route("/api/auth/me", get(me_handler))
        .with_state(state)
}

pub async fn login_handler(
    State(state): State<Arc<AppState>>,
    session: Session,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let pool = state.db()?;

    let request_id = extract_request_id_from_headers(&headers);

    // Check brute force lockout before attempting authentication
    check_login_lockout(&state, &payload.email).await?;

    let user = match auth::find_user_by_email(pool, &payload.email).await? {
        Some(u) => u,
        None => {
            record_failed_login(&state, &payload.email).await;
            let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
                actor_id: None,
                action: "login.failed".to_string(),
                target: payload.email.clone(),
                description: Some("Failed login attempt: user not found".to_string()),
                metadata: serde_json::json!({}),
                request_id: Some(request_id.clone()),
            }]).await;
            return Err(AppError::Unauthorized("Invalid email or password".to_string()));
        }
    };

    let valid = auth::verify_password(&payload.password, &user.password_hash).await?;
    if !valid {
        record_failed_login(&state, &payload.email).await;
        let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
            actor_id: Some(user.id),
            action: "login.failed".to_string(),
            target: user.email.clone(),
            description: Some("Failed login attempt: invalid password".to_string()),
            metadata: serde_json::json!({}),
            request_id: Some(request_id.clone()),
        }]).await;
        return Err(AppError::Unauthorized("Invalid email or password".to_string()));
    }

    // Clear failure counter on successful login
    clear_login_failures(&state, &payload.email).await;

    // Rotate session to prevent fixation
    let _ = session.delete().await;

    session.insert(SESSION_USER_ID_KEY, user.id).await
        .map_err(|e| AppError::Internal(format!("Session error: {}", e)))?;

    auth::update_last_login(pool, user.id).await?;

    let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
        actor_id: Some(user.id),
        action: "login.success".to_string(),
        target: user.email.clone(),
        description: Some("Successful login".to_string()),
        metadata: serde_json::json!({}),
        request_id: Some(request_id),
    }]).await;

    Ok(Json(LoginResponse {
        user: user.to_public(),
    }))
}

const LOGIN_FAIL_PREFIX: &str = "login_fail:";
const LOGIN_FAIL_TTL: u64 = 900;
const MAX_LOGIN_ATTEMPTS: u32 = 5;

async fn check_login_lockout(state: &Arc<AppState>, email: &str) -> Result<(), AppError> {
    let key = format!("{}{}", LOGIN_FAIL_PREFIX, email);
    if let Some(ref redis) = state.rate_limit_redis {
        let mut conn = redis.lock().await;
        let count: Option<u32> = match redis::cmd("GET")
            .arg(&key)
            .query_async(&mut *conn)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("[AUTH] Redis error in login lockout check: {}", e);
                // Fail closed on Redis errors to prevent brute-force bypass
                return Err(AppError::TooManyRequests(
                    "Rate limiting unavailable. Try again later.".to_string()
                ));
            }
        };
        if let Some(count) = count {
            if count >= MAX_LOGIN_ATTEMPTS {
                let ttl: i64 = redis::cmd("TTL")
                    .arg(&key)
                    .query_async(&mut *conn)
                    .await
                    .unwrap_or(900);
                return Err(AppError::TooManyRequests(format!(
                    "Too many login attempts. Try again in {} seconds.",
                    ttl,
                )));
            }
        }
    }
    Ok(())
}

async fn record_failed_login(state: &Arc<AppState>, email: &str) {
    let key = format!("{}{}", LOGIN_FAIL_PREFIX, email);
    if let Some(ref redis) = state.rate_limit_redis {
        let mut conn = redis.lock().await;
        let count: u64 = redis::cmd("INCR")
            .arg(&key)
            .query_async(&mut *conn)
            .await
            .unwrap_or(0);
        if count == 1 {
            let _: () = redis::cmd("EXPIRE")
                .arg(&key)
                .arg(LOGIN_FAIL_TTL as i64)
                .query_async(&mut *conn)
                .await
                .unwrap_or(());
        }
    }
}

async fn clear_login_failures(state: &Arc<AppState>, email: &str) {
    let key = format!("{}{}", LOGIN_FAIL_PREFIX, email);
    if let Some(ref redis) = state.rate_limit_redis {
        let mut conn = redis.lock().await;
        let _: () = redis::cmd("DEL")
            .arg(&key)
            .query_async(&mut *conn)
            .await
            .unwrap_or(());
    }
}

pub async fn logout_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    session: Session,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id: Option<uuid::Uuid> = session.get(SESSION_USER_ID_KEY).await.unwrap_or(None);
    let email: Option<String> = if let Some(uid) = user_id {
        if let Some(pool) = state.db_pool.as_ref() {
            auth::find_user_by_id(pool, uid).await.ok().flatten().map(|u| u.email)
        } else {
            None
        }
    } else {
        None
    };

    session.delete().await
        .map_err(|e| AppError::Internal(format!("Session error: {}", e)))?;

    if let Some(ref pool) = state.db_pool {
        let request_id = extract_request_id_from_headers(&headers);
        let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
            actor_id: user_id,
            action: "logout".to_string(),
            target: email.unwrap_or_else(|| "unknown".to_string()),
            description: Some("User logged out".to_string()),
            metadata: serde_json::json!({}),
            request_id: Some(request_id),
        }]).await;
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn me_handler(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MeResponse>, AppError> {
    let pool = state.db()?;

    let user_id: uuid::Uuid = session.get(SESSION_USER_ID_KEY).await
        .map_err(|e| AppError::Internal(format!("Session error: {}", e)))?
        .ok_or_else(|| AppError::Unauthorized("Not authenticated".to_string()))?;

    let user = auth::find_user_by_id(pool, user_id).await?
        .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    let scopes: Vec<String> = sqlx::query_scalar(
        r#"SELECT DISTINCT rs.scope
           FROM user_roles ur
           JOIN role_scopes rs ON rs.role_id = ur.role_id
           WHERE ur.user_id = $1
           ORDER BY rs.scope"#
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    Ok(Json(MeResponse {
        user: user.to_public(),
        scopes,
    }))
}
