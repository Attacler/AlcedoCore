use crate::db::queries::PluginRecovery;
use crate::error::AppError;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::time::Duration;

const DEFAULT_MAX_RESTART_ATTEMPTS: i32 = 10;

#[derive(Debug, Clone)]
pub struct RecoveryState {
    pub install_id: i64,
    pub restart_count: i32,
    pub last_restart_at: Option<DateTime<Utc>>,
    pub backoff_seconds: u64,
}

impl RecoveryState {
    pub fn new(install_id: i64) -> Self {
        Self {
            install_id,
            restart_count: 0,
            last_restart_at: None,
            backoff_seconds: 1,
        }
    }

    pub fn from_db(recovery: &PluginRecovery) -> Self {
        Self {
            install_id: recovery.install_id,
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

pub async fn should_restart(
    db: &PgPool,
    install_id: i64,
    max_attempts: i32,
) -> Result<bool, AppError> {
    match PluginRecovery::find_by_install(db, install_id).await? {
        Some(recovery) => Ok(recovery.restart_count < max_attempts),
        None => Ok(true),
    }
}

pub async fn record_restart(
    db: &PgPool,
    install_id: i64,
    last_error: Option<&str>,
) -> Result<(), AppError> {
    let recovery = match PluginRecovery::find_by_install(db, install_id).await? {
        Some(r) => r,
        None => {
            let new_recovery = PluginRecovery {
                install_id,
                restart_count: 0,
                last_restart_at: None,
                restart_policy: "always".to_string(),
                next_restart_at: None,
                max_restart_attempts: DEFAULT_MAX_RESTART_ATTEMPTS,
                last_error: None,
            };
            PluginRecovery::insert(db, &new_recovery).await?;
            PluginRecovery::find_by_install(db, install_id)
                .await?
                .ok_or_else(|| {
                    AppError::Internal(format!(
                        "Failed to create recovery record for plugin install {}",
                        install_id
                    ))
                })?
        }
    };

    let next_restart_at = Utc::now()
        + chrono::Duration::seconds(
            get_backoff_delay(recovery.restart_count as u8 + 1).as_secs() as i64,
        );

    PluginRecovery::record_restart(
        db,
        install_id,
        recovery.restart_count + 1,
        Utc::now(),
        Some(next_restart_at),
        last_error,
    )
    .await
}

pub async fn clear_restart(db: &PgPool, install_id: i64) -> Result<(), AppError> {
    match PluginRecovery::find_by_install(db, install_id).await? {
        Some(_) => PluginRecovery::clear_restart(db, install_id).await,
        None => Ok(()),
    }
}

pub fn get_max_restart_attempts() -> i32 {
    DEFAULT_MAX_RESTART_ATTEMPTS
}

/// Find slug by deployment_id from alcedo_plugin_versions table
pub async fn find_slug_by_deployment_id(
    db: &PgPool,
    deployment_id: &str,
) -> Result<Option<String>, AppError> {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT slug FROM alcedo_plugin_versions WHERE deployment_id = $1 LIMIT 1",
    )
    .bind(deployment_id)
    .fetch_optional(db)
    .await?;
    Ok(result)
}

/// Find the plugin install_id by deployment_id from alcedo_plugin_versions table
pub async fn find_install_id_by_deployment_id(
    db: &PgPool,
    deployment_id: &str,
) -> Result<Option<i64>, AppError> {
    let result = sqlx::query_scalar::<_, i64>(
        "SELECT install_id FROM alcedo_plugin_versions WHERE deployment_id = $1 LIMIT 1",
    )
    .bind(deployment_id)
    .fetch_optional(db)
    .await?;
    Ok(result)
}
