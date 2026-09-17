use crate::{error::AppError, find_all, find_all_where};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct PluginRecovery {
    pub install_id: i64,
    pub restart_count: i32,
    pub last_restart_at: Option<chrono::DateTime<chrono::Utc>>,
    pub restart_policy: String,
    pub next_restart_at: Option<chrono::DateTime<chrono::Utc>>,
    pub max_restart_attempts: i32,
    pub last_error: Option<String>,
}

impl PluginRecovery {
    /// Find a recovery record by plugin install id
    pub async fn find_by_install(
        db: &PgPool,
        install_id: i64,
    ) -> Result<Option<Self>, AppError> {
        let row = sqlx::query_as::<_, Self>(
            "SELECT install_id, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error
             FROM alcedo_plugin_recovery
             WHERE install_id = $1",
        )
        .bind(install_id)
        .fetch_optional(db)
        .await?;
        Ok(row)
    }

    /// Insert a new recovery record
    pub async fn insert(db: &PgPool, recovery: &PluginRecovery) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO alcedo_plugin_recovery (install_id, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error)
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        )
        .bind(recovery.install_id)
        .bind(recovery.restart_count)
        .bind(&recovery.last_restart_at)
        .bind(&recovery.restart_policy)
        .bind(&recovery.next_restart_at)
        .bind(recovery.max_restart_attempts)
        .bind(&recovery.last_error)
        .execute(db)
        .await?;
        Ok(())
    }

    /// Update restart count and last restart timestamp
    pub async fn record_restart(
        db: &PgPool,
        install_id: i64,
        restart_count: i32,
        last_restart_at: chrono::DateTime<chrono::Utc>,
        next_restart_at: Option<chrono::DateTime<chrono::Utc>>,
        last_error: Option<&str>,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE alcedo_plugin_recovery
             SET restart_count = $2, last_restart_at = $3, next_restart_at = $4, last_error = $5
             WHERE install_id = $1",
        )
        .bind(install_id)
        .bind(restart_count)
        .bind(last_restart_at)
        .bind(next_restart_at)
        .bind(last_error)
        .execute(db)
        .await?;
        Ok(())
    }

    /// Clear restart state after successful restart
    pub async fn clear_restart(db: &PgPool, install_id: i64) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE alcedo_plugin_recovery
             SET restart_count = 0, last_restart_at = NULL, next_restart_at = NULL, last_error = NULL
             WHERE install_id = $1"
        )
        .bind(install_id)
        .execute(db)
        .await?;
        Ok(())
    }

    find_all_where!(find_due_for_restart, "alcedo_plugin_recovery", "install_id, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error", "next_restart_at IS NOT NULL AND next_restart_at <= NOW() AND restart_count < max_restart_attempts", "install_id");
    find_all_where!(find_crash_loops, "alcedo_plugin_recovery", "install_id, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error", "restart_count >= max_restart_attempts", "install_id");

    find_all!(find_all, "alcedo_plugin_recovery", "install_id, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error", "install_id");
}
