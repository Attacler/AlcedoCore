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
           FROM alcedo_users WHERE email = $1"#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"SELECT id, email, password_hash, display_name, is_admin, created_at, updated_at
           FROM alcedo_users WHERE id = $1"#,
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
        r#"INSERT INTO alcedo_users (email, password_hash, display_name, is_admin)
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
    sqlx::query("UPDATE alcedo_users SET last_login_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Provision a developer API key for testing. If `key` is None, a random key is generated.
/// Returns the provisioned key value.
///
/// Developer API keys are version-scoped, so the key is attached to the
/// `default`/`v1` version. The `default`/`v1` rows are seeded here when
/// absent so the harness can provision a key without extra setup.
pub async fn provision_dev_api_key(pool: &PgPool, key: Option<String>) -> Result<String, AppError> {
    let key = key.unwrap_or_else(|| format!("dev_{}", uuid::Uuid::new_v4().to_string().replace("-", "")));
    let password_hash = hash_password(&key).await?;
    let key_prefix = key[..key.len().min(10)].to_string();
    let version_id = ensure_default_version(pool).await?;
    crate::db::queries::DeveloperApiKey::insert(
        pool,
        "dev-test-key",
        version_id,
        &password_hash,
        &key_prefix,
    )
    .await?;
    Ok(key)
}

/// Resolve the `default`/`v1` version id (`alcedo_versions.id`), seeding the
/// app and app×version registry rows when they don't exist yet. Dev keys are
/// version-scoped, so this is the id they attach to.
pub async fn ensure_default_version(pool: &PgPool) -> Result<i32, AppError> {
    let app_version_id = ensure_default_app_version(pool).await?;
    let version_id: Option<(i32,)> =
        sqlx::query_as(r#"SELECT version_id FROM alcedo.alcedo_apps_versions WHERE id = $1"#)
            .bind(app_version_id)
            .fetch_optional(pool)
            .await?;
    version_id
        .map(|r| r.0)
        .ok_or_else(|| AppError::Internal("default version not resolvable".to_string()))
}

/// Resolve the `default`/`v1` app×version id, seeding the registry rows when
/// they don't exist yet. Mirrors the app migration runner's source tables.
pub async fn ensure_default_app_version(pool: &PgPool) -> Result<i32, AppError> {
    let existing: Option<(i32,)> = sqlx::query_as(
        r#"SELECT av.id FROM alcedo.alcedo_apps_versions av
           JOIN alcedo.alcedo_apps a ON a.id = av.app_id
           JOIN alcedo.alcedo_versions v ON v.id = av.version_id
           WHERE a.api_name = $1 AND v.version_name = $2"#,
    )
    .bind(alcedo_common::context::DEFAULT_APP_NAME)
    .bind("v1")
    .fetch_optional(pool)
    .await?;
    if let Some((id,)) = existing {
        return Ok(id);
    }

    sqlx::query(
        r#"INSERT INTO alcedo.alcedo_apps (name, api_name)
           SELECT $1, $1
           WHERE NOT EXISTS (SELECT 1 FROM alcedo.alcedo_apps WHERE api_name = $1)"#,
    )
    .bind(alcedo_common::context::DEFAULT_APP_NAME)
    .execute(pool)
    .await?;
    sqlx::query(
        r#"INSERT INTO alcedo.alcedo_versions (version_name)
           SELECT $1
           WHERE NOT EXISTS (SELECT 1 FROM alcedo.alcedo_versions WHERE version_name = $1)"#,
    )
    .bind("v1")
    .execute(pool)
    .await?;
    let inserted: Option<(i32,)> = sqlx::query_as(
        r#"INSERT INTO alcedo.alcedo_apps_versions (app_id, version_id)
           SELECT a.id, v.id FROM alcedo.alcedo_apps a, alcedo.alcedo_versions v
           WHERE a.api_name = $1 AND v.version_name = $2
             AND NOT EXISTS (
                 SELECT 1 FROM alcedo.alcedo_apps_versions av
                 JOIN alcedo.alcedo_apps aa ON aa.id = av.app_id
                 JOIN alcedo.alcedo_versions vv ON vv.id = av.version_id
                 WHERE aa.api_name = $1 AND vv.version_name = $2
             )
           RETURNING id"#,
    )
    .bind(alcedo_common::context::DEFAULT_APP_NAME)
    .bind("v1")
    .fetch_optional(pool)
    .await?;
    if let Some((id,)) = inserted {
        return Ok(id);
    }

    let existing: Option<(i32,)> = sqlx::query_as(
        r#"SELECT av.id FROM alcedo.alcedo_apps_versions av
           JOIN alcedo.alcedo_apps a ON a.id = av.app_id
           JOIN alcedo.alcedo_versions v ON v.id = av.version_id
           WHERE a.api_name = $1 AND v.version_name = $2
           ORDER BY av.id ASC"#,
    )
    .bind(alcedo_common::context::DEFAULT_APP_NAME)
    .bind("v1")
    .fetch_optional(pool)
    .await?;
    existing
        .map(|r| r.0)
        .ok_or_else(|| AppError::Internal("default app/version not resolvable".to_string()))
}
