use crate::{error::AppError, find_all_where_bind, find_by_where};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct DeveloperApiKey {
    pub id: uuid::Uuid,
    pub name: String,
    pub version_id: i32,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub key_prefix: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

macro_rules! key_columns {
    () => {
        "id, name, version_id, key_hash, key_prefix, is_active, created_at, last_used_at"
    };
}

impl DeveloperApiKey {
    find_by_where!(
        find_by_hash,
        "alcedo_developer_api_keys",
        key_columns!(),
        "key_hash",
        "is_active = true"
    );
    find_all_where_bind!(
        find_by_prefix,
        "alcedo_developer_api_keys",
        key_columns!(),
        "key_prefix = $1 AND is_active = true",
        "created_at DESC"
    );

    pub async fn find_by_prefix_and_version(
        db: &PgPool,
        prefix: &str,
        version_id: i32,
    ) -> Result<Vec<DeveloperApiKey>, AppError> {
        let rows = sqlx::query_as::<_, DeveloperApiKey>(concat!(
            "SELECT ",
            key_columns!(),
            " FROM alcedo_developer_api_keys
             WHERE key_prefix = $1 AND version_id = $2 AND is_active = true
             ORDER BY created_at DESC"
        ))
        .bind(prefix)
        .bind(version_id)
        .fetch_all(db)
        .await?;
        Ok(rows)
    }

    pub async fn insert(
        db: &PgPool,
        name: &str,
        version_id: i32,
        key_hash: &str,
        key_prefix: &str,
    ) -> Result<DeveloperApiKey, AppError> {
        let row = sqlx::query_as::<_, DeveloperApiKey>(concat!(
            "INSERT INTO alcedo_developer_api_keys (name, version_id, key_hash, key_prefix)
             VALUES ($1, $2, $3, $4)
             RETURNING ",
            key_columns!()
        ))
        .bind(name)
        .bind(version_id)
        .bind(key_hash)
        .bind(key_prefix)
        .fetch_one(db)
        .await?;
        Ok(row)
    }

    pub async fn touch_last_used(db: &PgPool, id: uuid::Uuid) -> Result<(), AppError> {
        sqlx::query("UPDATE alcedo_developer_api_keys SET last_used_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(db)
            .await?;
        Ok(())
    }
}
