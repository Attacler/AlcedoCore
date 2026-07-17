use crate::{error::AppError, find_all, find_by};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct SystemSetting {
    pub key: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl SystemSetting {
    find_all!(find_all, "system_settings", "key, value, description, updated_at", "key");
    find_by!(find_by_key, "system_settings", "key, value, description, updated_at", "key");

    pub async fn upsert(
        db: &PgPool,
        key: &str,
        value: &serde_json::Value,
        description: Option<&str>,
    ) -> Result<SystemSetting, AppError> {
        let row = sqlx::query_as::<_, SystemSetting>(
            "INSERT INTO system_settings (key, value, description, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT (key) DO UPDATE SET
               value = EXCLUDED.value,
               description = COALESCE($3, system_settings.description),
               updated_at = NOW()
             RETURNING key, value, description, updated_at"
        )
        .bind(key)
        .bind(value)
        .bind(description)
        .fetch_one(db)
        .await?;
        Ok(row)
    }

    pub async fn upsert_batch(
        db: &PgPool,
        settings: Vec<(String, serde_json::Value)>,
    ) -> Result<Vec<SystemSetting>, AppError> {
        let mut tx = db.begin().await?;
        let mut results = Vec::with_capacity(settings.len());
        for (key, value) in settings {
            let result = sqlx::query_as::<_, SystemSetting>(
                "INSERT INTO system_settings (key, value, updated_at)
                 VALUES ($1, $2, NOW())
                 ON CONFLICT (key) DO UPDATE SET
                   value = EXCLUDED.value,
                   updated_at = NOW()
                 RETURNING key, value, description, updated_at"
            )
            .bind(&key)
            .bind(&value)
            .fetch_one(&mut *tx)
            .await?;
            results.push(result);
        }
        tx.commit().await?;
        Ok(results)
    }
}
