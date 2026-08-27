use std::time::Duration;
use sqlx::PgPool;
use chrono::{DateTime, Utc};
use crate::db::queries::PluginRecovery;
use crate::error::AppError;

const DEFAULT_MAX_RESTART_ATTEMPTS: i32 = 10;

#[derive(Debug, Clone)]
pub struct RecoveryState {
    pub slug: String,
    pub restart_count: i32,
    pub last_restart_at: Option<DateTime<Utc>>,
    pub backoff_seconds: u64,
}

impl RecoveryState {
    pub fn new(slug: String) -> Self {
        Self {
            slug,
            restart_count: 0,
            last_restart_at: None,
            backoff_seconds: 1,
        }
    }

    pub fn from_db(recovery: &PluginRecovery) -> Self {
        Self {
            slug: recovery.slug.clone(),
            restart_count: recovery.restart_count,
            last_restart_at: recovery.last_restart_at,
            backoff_seconds: get_backoff_delay(recovery.restart_count as u8).as_secs(),
        }
    }

    pub fn should_restart(&self, max_attempts: i32) -> bool {
        self.restart_count < max_attempts
    }

    pub fn record_restart(&mut self) {
        self.restart_count += 1;
        self.last_restart_at = Some(Utc::now());
        self.backoff_seconds = get_backoff_delay(self.restart_count as u8).as_secs();
    }

    pub fn clear(&mut self) {
        self.restart_count = 0;
        self.last_restart_at = None;
        self.backoff_seconds = 1;
    }
}

pub fn get_backoff_delay(restart_count: u8) -> Duration {
    match restart_count {
        0 => Duration::from_secs(1),
        1 => Duration::from_secs(2),
        _ => Duration::from_secs(4),
    }
}

pub async fn should_restart(db: &PgPool, slug: &str, max_attempts: i32) -> Result<bool, AppError> {
    match PluginRecovery::find_by_slug(db, slug).await? {
        Some(recovery) => Ok(recovery.restart_count < max_attempts),
        None => Ok(true),
    }
}

pub async fn record_restart(
    db: &PgPool,
    slug: &str,
    last_error: Option<&str>,
) -> Result<(), AppError> {
    let recovery = match PluginRecovery::find_by_slug(db, slug).await? {
        Some(r) => r,
        None => {
            let new_recovery = PluginRecovery {
                slug: slug.to_string(),
                restart_count: 0,
                last_restart_at: None,
                restart_policy: "always".to_string(),
                next_restart_at: None,
                max_restart_attempts: DEFAULT_MAX_RESTART_ATTEMPTS,
                last_error: None,
            };
            PluginRecovery::insert(db, &new_recovery).await?;
            PluginRecovery::find_by_slug(db, slug).await?
                .ok_or_else(|| AppError::Internal(
                    format!("Failed to create recovery record for plugin {}", slug)
                ))?
        }
    };

    let next_restart_at = Utc::now() + chrono::Duration::seconds(get_backoff_delay(recovery.restart_count as u8 + 1).as_secs() as i64);

    PluginRecovery::record_restart(
        db,
        slug,
        recovery.restart_count + 1,
        Utc::now(),
        Some(next_restart_at),
        last_error,
    )
    .await
}

pub async fn clear_restart(db: &PgPool, slug: &str) -> Result<(), AppError> {
    match PluginRecovery::find_by_slug(db, slug).await? {
        Some(_) => PluginRecovery::clear_restart(db, slug).await,
        None => Ok(()),
    }
}

pub fn get_max_restart_attempts() -> i32 {
    DEFAULT_MAX_RESTART_ATTEMPTS
}

/// Find slug by container_id from plugin_versions table
pub async fn find_slug_by_container_id(db: &PgPool, container_id: &str) -> Result<Option<String>, AppError> {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT slug FROM plugin_versions WHERE container_id = $1 LIMIT 1"
    )
    .bind(container_id)
    .fetch_optional(db)
    .await?;
    Ok(result)
}