//! Crash recovery for plugin installs.
//!
//! Ported from v1's `alcedo-db/src/db/resilience.rs`. The
//! `alcedocore_plugin_recovery` table tracks restart attempts per install so
//! the Docker event watcher can apply exponential backoff and give up after
//! `max_restart_attempts`.
//!
//! Only called from `docker_platform.rs` (behind `#[cfg(feature = "docker")]`),
//! so without the docker feature everything here looks unused.

use std::time::Duration;

use chrono::Utc;
use sqlx::{Pool, Postgres, Row};

use crate::services::errors::AlcedoError;

pub const DEFAULT_MAX_RESTART_ATTEMPTS: i32 = 10;

/// Exponential backoff: 1s → 2s → 4s (capped).
pub fn get_backoff_delay(restart_count: u8) -> Duration {
    match restart_count {
        0 => Duration::from_secs(1),
        1 => Duration::from_secs(2),
        _ => Duration::from_secs(4),
    }
}

/// Whether the install is still eligible for an automatic restart.
pub async fn should_restart(
    pool: &Pool<Postgres>,
    install_id: i64,
    max_attempts: i32,
) -> Result<bool, AlcedoError> {
    let row = sqlx::query(
        "SELECT restart_count FROM alcedocore.alcedocore_plugin_recovery WHERE install_id = $1",
    )
    .bind(install_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => {
            let count: i32 = row.try_get("restart_count")?;

            Ok(count < max_attempts)
        }
        None => Ok(true),
    }
}

/// Record a restart attempt: increment the counter, set `last_restart_at`,
/// and compute `next_restart_at` from the backoff delay.
pub async fn record_restart(
    pool: &Pool<Postgres>,
    install_id: i64,
    last_error: Option<&str>,
) -> Result<(), AlcedoError> {
    let existing = sqlx::query(
        "SELECT restart_count FROM alcedocore.alcedocore_plugin_recovery WHERE install_id = $1",
    )
    .bind(install_id)
    .fetch_optional(pool)
    .await?;

    let current_count: i32 = existing
        .as_ref()
        .map(|row| row.try_get("restart_count"))
        .transpose()?
        .unwrap_or(0);

    let new_count = current_count + 1;
    let next_restart_at =
        Utc::now() + chrono::Duration::seconds(get_backoff_delay(new_count as u8).as_secs() as i64);

    sqlx::query(
        "INSERT INTO alcedocore.alcedocore_plugin_recovery \
         (install_id, restart_count, last_restart_at, restart_policy, next_restart_at, \
          max_restart_attempts, last_error) \
         VALUES ($1, $2, NOW(), 'always', $3, $4, $5) \
         ON CONFLICT (install_id) DO UPDATE SET \
             restart_count = EXCLUDED.restart_count, \
             last_restart_at = EXCLUDED.last_restart_at, \
             next_restart_at = EXCLUDED.next_restart_at, \
             last_error = EXCLUDED.last_error",
    )
    .bind(install_id)
    .bind(new_count)
    .bind(next_restart_at)
    .bind(DEFAULT_MAX_RESTART_ATTEMPTS)
    .bind(last_error)
    .execute(pool)
    .await?;

    Ok(())
}

/// Reset the restart counter — called when a container starts successfully.
pub async fn clear_restart(pool: &Pool<Postgres>, install_id: i64) -> Result<(), AlcedoError> {
    sqlx::query(
        "UPDATE alcedocore.alcedocore_plugin_recovery \
         SET restart_count = 0, last_restart_at = NULL, next_restart_at = NULL, last_error = NULL \
         WHERE install_id = $1",
    )
    .bind(install_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Current restart count for an install (0 if no row exists).
pub async fn get_restart_count(
    pool: &Pool<Postgres>,
    install_id: i64,
) -> Result<i32, AlcedoError> {
    let count: Option<i32> = sqlx::query_scalar(
        "SELECT restart_count FROM alcedocore.alcedocore_plugin_recovery WHERE install_id = $1",
    )
    .bind(install_id)
    .fetch_optional(pool)
    .await?;
    Ok(count.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::get_backoff_delay;

    #[test]
    fn backoff_is_exponential_and_capped() {
        assert_eq!(get_backoff_delay(0).as_secs(), 1);
        assert_eq!(get_backoff_delay(1).as_secs(), 2);
        assert_eq!(get_backoff_delay(2).as_secs(), 4);
        assert_eq!(get_backoff_delay(5).as_secs(), 4);
        assert_eq!(get_backoff_delay(255).as_secs(), 4);
    }
}
