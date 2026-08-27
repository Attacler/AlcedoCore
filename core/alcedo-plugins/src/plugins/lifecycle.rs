use crate::error::AppError;

/// Graceful shutdown - plugins are independent services and should not be
/// stopped when the core shuts down. They continue serving through Swarm
/// and reconnect automatically when the core restarts.
pub async fn graceful_shutdown() -> Result<(), AppError> {
    tracing::info!("Core shutting down — plugin containers left running (they are independent services)");
    Ok(())
}