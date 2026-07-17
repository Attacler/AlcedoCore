use crate::{error::AppError, find_all, find_all_where, find_by};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct PluginRecovery {
    pub slug: String,
    pub restart_count: i32,
    pub last_restart_at: Option<chrono::DateTime<chrono::Utc>>,
    pub restart_policy: String,
    pub next_restart_at: Option<chrono::DateTime<chrono::Utc>>,
    pub max_restart_attempts: i32,
    pub last_error: Option<String>,
}

impl PluginRecovery {
    find_by!(find_by_slug, "plugin_recovery", "slug, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error", "slug");

    /// Insert a new recovery record
    pub async fn insert(db: &PgPool, recovery: &PluginRecovery) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO plugin_recovery (slug, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error)
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        )
        .bind(&recovery.slug)
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
        slug: &str,
        restart_count: i32,
        last_restart_at: chrono::DateTime<chrono::Utc>,
        next_restart_at: Option<chrono::DateTime<chrono::Utc>>,
        last_error: Option<&str>,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE plugin_recovery
             SET restart_count = $2, last_restart_at = $3, next_restart_at = $4, last_error = $5
             WHERE slug = $1"
        )
        .bind(slug)
        .bind(restart_count)
        .bind(last_restart_at)
        .bind(next_restart_at)
        .bind(last_error)
        .execute(db)
        .await?;
        Ok(())
    }

    /// Clear restart state after successful restart
    pub async fn clear_restart(db: &PgPool, slug: &str) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE plugin_recovery
             SET restart_count = 0, last_restart_at = NULL, next_restart_at = NULL, last_error = NULL
             WHERE slug = $1"
        )
        .bind(slug)
        .execute(db)
        .await?;
        Ok(())
    }

    find_all_where!(find_due_for_restart, "plugin_recovery", "slug, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error", "next_restart_at IS NOT NULL AND next_restart_at <= NOW() AND restart_count < max_restart_attempts", "slug");
    find_all_where!(find_crash_loops, "plugin_recovery", "slug, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error", "restart_count >= max_restart_attempts", "slug");

    find_all!(find_all, "plugin_recovery", "slug, restart_count, last_restart_at, restart_policy, next_restart_at, max_restart_attempts, last_error", "slug");
}
