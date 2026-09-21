use std::{collections::HashMap, iter::Map, time::Duration};

use argon2::PasswordVerifier;
use argon2::password_hash::PasswordHasher;
use argon2::{Argon2, PasswordHash};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::spawn_blocking;
use uuid::Uuid;

use crate::services::postgres::pool::execute_query;
use crate::services::{
    app_state::AppState,
    context::AppContext,
    errors::AlcedoError,
    items::{
        query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query},
        service::ItemsService,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlcedoUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing)]
    pub password_hash: String,
}

const LOGIN_FAIL_PREFIX: &str = "login_fail:";
pub struct AuthService<'a> {
    app_state: &'a AppState,
    app_context: &'a AppContext,
    collection: String,
}

impl AuthService<'_> {
    pub fn new<'a>(app_state: &'a AppState, context: &'a AppContext) -> AuthService<'a> {
        return AuthService {
            app_state,
            app_context: context,
            collection: "alcedo_users".to_string(),
        };
    }

    pub async fn find_user_by_email(&self, email: &str) -> Result<Option<AlcedoUser>, AlcedoError> {
        if email.len() == 0 {
            return Ok(None);
        }
        let service = ItemsService::new(&self.app_state, &self.app_context, &self.collection);

        let mut query = Query::default();
        let mut hmap = FieldFilter {
            fields: HashMap::new(),
        };

        hmap.fields.insert(
            "email".to_string(),
            FieldValue::Comparison(Comparison {
                _eq: Some(email.into()),
                ..Default::default()
            }),
        );
        query.filter = LogicOp {
            _and: Some(vec![Filter::Field(hmap)]),
            _or: None,
        };
        let user = service.read_items_by_query(query).await?;

        if user.len() == 0 {
            return Ok(None);
        }
        let user = user.get(0).unwrap();
        let user: AlcedoUser = Value::Object(user.clone()).try_into()?;

        Ok(Some(user))
    }

    pub async fn check_login_lockout(&self, email: &str) -> Result<(), AlcedoError> {
        let key = format!("{}{}", LOGIN_FAIL_PREFIX, email);

        let count = match self.app_state.cache.get(&key).await {
            Ok(Some(s)) => match s.parse::<u32>() {
                Ok(n) => Some(n),
                Err(_) => {
                    tracing::error!("[AUTH] corrupt login_fail counter for {}", key);
                    return Err(AlcedoError::SystemError(
                        "Rate limiting unavailable. Try again later.".to_string(),
                        0,
                    ));
                }
            },
            Ok(None) => None,
            Err(e) => {
                tracing::error!("[AUTH] Redis error in login lockout check: {}", e);
                return Err(AlcedoError::SystemError(
                    "Rate limiting unavailable. Try again later.".to_string(),
                    0,
                ));
            }
        };
        if let Some(count) = count {
            if count >= self.app_state.config.max_login_attempts.into() {
                self.app_state
                    .cache
                    .set_ttl(
                        key,
                        format!("{}", count + 1),
                        Duration::from_secs(self.app_state.config.login_fatal_ttl.into()),
                    )
                    .await?;
                return Err(AlcedoError::TooManyRequests());
            }
        }

        Ok(())
    }

    pub async fn record_failed_login(&self, email: &str) {
        let key = format!("{}{}", LOGIN_FAIL_PREFIX, email);

        self.app_state
            .cache
            .incr_with_ttl(
                &key,
                Duration::from_secs(self.app_state.config.login_fatal_ttl.into()),
            )
            .await;
    }

    pub async fn clear_login_failures(&self, email: &str) {
        let key = format!("{}{}", LOGIN_FAIL_PREFIX, email);
        self.app_state.cache.del(&key).await;
    }

    pub async fn update_last_login(&self, user_id: Uuid) -> Result<(), AlcedoError> {
        execute_query(
            &self.app_state,
            format!(
                "UPDATE alcedo.alcedo_users SET last_login = NOW() WHERE id = '{}'",
                user_id
            ),
        )
        .await?;
        Ok(())
    }

    pub async fn hash_password(password: &str) -> Result<String, AlcedoError> {
        let password = password.to_string();
        spawn_blocking(move || {
            Argon2::default()
                .hash_password(password.as_bytes())
                .map(|hash| hash.to_string())
                .map_err(|e| AlcedoError::SystemError(format!("Password hashing failed: {}", e), 0))
        })
        .await
        .map_err(|e| AlcedoError::SystemError(format!("Password hashing task failed: {}", e), 0))?
    }

    pub async fn verify_password(password: &str, hash: &str) -> Result<bool, AlcedoError> {
        let password = password.to_string();
        let hash = hash.to_string();
        spawn_blocking(move || {
            let parsed_hash = PasswordHash::new(&hash).map_err(|e| {
                AlcedoError::SystemError(format!("Invalid password hash: {}", e), 0)
            })?;
            Ok(Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .map(|_| true)
                .or_else(|e| match e {
                    argon2::password_hash::Error::PasswordInvalid => Ok(false),
                    other => Err(AlcedoError::SystemError(
                        format!("Verify error: {}", other),
                        0,
                    )),
                })?)
        })
        .await
        .map_err(|e| {
            AlcedoError::SystemError(format!("Password verification task failed: {}", e), 0)
        })?
    }
}

fn parse_timestamp(value: Option<&Value>) -> Result<DateTime<Utc>, AlcedoError> {
    let raw = value
        .and_then(|v| v.as_str())
        .ok_or_else(|| AlcedoError::SystemError("missing or invalid timestamp".to_string(), 0))?;

    if let Ok(datetime) = DateTime::parse_from_rfc3339(raw) {
        return Ok(datetime.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(naive.and_utc());
    }

    Err(AlcedoError::SystemError(
        format!("Invalid timestamp: {}", raw),
        0,
    ))
}

impl TryFrom<Value> for AlcedoUser {
    type Error = AlcedoError;

    fn try_from(user: Value) -> Result<Self, Self::Error> {
        Ok(AlcedoUser {
            id: Uuid::parse_str(user.get("id").and_then(|v| v.as_str()).ok_or_else(|| {
                AlcedoError::SystemError("missing or invalid 'id'".to_string(), 0)
            })?)
            .map_err(|e| AlcedoError::SystemError(format!("invalid 'id': {}", e), 0))?,
            email: user
                .get("email")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AlcedoError::SystemError("missing or invalid 'email'".to_string(), 0)
                })?
                .to_string(),
            display_name: user
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            is_admin: user
                .get("is_admin")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            created_at: parse_timestamp(user.get("created_at"))?,
            updated_at: parse_timestamp(user.get("updated_at"))?,
            password_hash: user
                .get("password_hash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }
}
