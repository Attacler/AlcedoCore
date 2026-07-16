use crate::{error::AppError, find_all, find_all_where_bind, find_by_where};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct DeveloperApiKey {
    pub id: uuid::Uuid,
    pub name: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub key_prefix: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl DeveloperApiKey {
    find_all!(list_all, "developer_api_keys", "id, name, key_hash, key_prefix, is_active, created_at, last_used_at", "created_at DESC");

    find_by_where!(find_by_hash, "developer_api_keys", "id, name, key_hash, key_prefix, is_active, created_at, last_used_at", "key_hash", "is_active = true");
    find_all_where_bind!(find_by_prefix, "developer_api_keys", "id, name, key_hash, key_prefix, is_active, created_at, last_used_at", "key_prefix = $1 AND is_active = true", "created_at DESC");

    pub async fn insert(db: &PgPool, name: &str, key_hash: &str, key_prefix: &str) -> Result<DeveloperApiKey, AppError> {
        let row = sqlx::query_as::<_, DeveloperApiKey>(
            "INSERT INTO developer_api_keys (name, key_hash, key_prefix)
             VALUES ($1, $2, $3)
             RETURNING id, name, key_hash, key_prefix, is_active, created_at, last_used_at"
        )
        .bind(name)
        .bind(key_hash)
        .bind(key_prefix)
        .fetch_one(db)
        .await?;
        Ok(row)
    }

    pub async fn delete(db: &PgPool, id: uuid::Uuid) -> Result<bool, AppError> {
        let rows = sqlx::query("DELETE FROM developer_api_keys WHERE id = $1")
            .bind(id)
            .execute(db)
            .await?
            .rows_affected();
        Ok(rows > 0)
    }

    pub async fn touch_last_used(db: &PgPool, id: uuid::Uuid) -> Result<(), AppError> {
        sqlx::query("UPDATE developer_api_keys SET last_used_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(db)
            .await?;
        Ok(())
    }
}
