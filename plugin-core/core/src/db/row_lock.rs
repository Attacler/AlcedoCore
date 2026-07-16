use crate::error::AppError;

/// Lock a collection by selecting its row FOR UPDATE within a transaction.
/// This blocks until the lock is acquired and releases on commit/rollback.
/// Unlike PostgreSQL advisory locks, row-level locks are transaction-scoped,
/// so unlock happens automatically at transaction end regardless of connection.
pub async fn lock_collection<'e, E>(
    executor: E,
    name: &str,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query("SELECT 1 FROM collection_definitions WHERE name = $1 FOR UPDATE")
        .bind(name)
        .execute(executor)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to lock collection '{}': {}", name, e),
        })?;
    Ok(())
}
