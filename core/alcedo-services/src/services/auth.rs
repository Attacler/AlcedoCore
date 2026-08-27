use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use sqlx::PgPool;
use tokio::task::spawn_blocking;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl User {
    pub fn to_public(&self) -> PublicUser {
        PublicUser {
            id: self.id,
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            is_admin: self.is_admin,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublicUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub async fn hash_password(password: &str) -> Result<String, AppError> {
    let password = password.to_string();
    spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| AppError::Internal(format!("Password hashing failed: {}", e)))?;
        Ok(hash.to_string())
    })
    .await
    .map_err(|e| AppError::Internal(format!("Password hashing task failed: {}", e)))?
}

pub async fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let password = password.to_string();
    let hash = hash.to_string();
    spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&hash)
            .map_err(|e| AppError::Internal(format!("Invalid password hash: {}", e)))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    })
    .await
    .map_err(|e| AppError::Internal(format!("Password verification task failed: {}", e)))?
}

pub async fn find_user_by_email(pool: &PgPool, email: &str) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"SELECT id, email, password_hash, display_name, is_admin, created_at, updated_at
           FROM users WHERE email = $1"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"SELECT id, email, password_hash, display_name, is_admin, created_at, updated_at
           FROM users WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn create_user(
    pool: &PgPool,
    email: &str,
    password: &str,
    display_name: Option<&str>,
    is_admin: bool,
) -> Result<User, AppError> {
    let password_hash = hash_password(password).await?;
    let user = sqlx::query_as::<_, User>(
        r#"INSERT INTO users (email, password_hash, display_name, is_admin)
           VALUES ($1, $2, $3, $4)
           RETURNING id, email, password_hash, display_name, is_admin, created_at, updated_at"#,
    )
    .bind(email)
    .bind(&password_hash)
    .bind(display_name)
    .bind(is_admin)
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn update_last_login(pool: &PgPool, user_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET last_login_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Provision a developer API key for testing. If `key` is None, a random key is generated.
/// Returns the provisioned key value.
pub async fn provision_dev_api_key(pool: &PgPool, key: Option<String>) -> Result<String, AppError> {
    let key = key.unwrap_or_else(|| format!("dev_{}", uuid::Uuid::new_v4().to_string().replace("-", "")));
    let password_hash = hash_password(&key).await?;
    let key_prefix = key[..key.len().min(10)].to_string();
    crate::db::queries::DeveloperApiKey::insert(pool, "dev-test-key", &password_hash, &key_prefix).await?;
    Ok(key)
}
