use std::collections::HashMap;
use std::time::Duration;
use futures_util::StreamExt;

use pcl::db::Pool;
use pcl::AppError;
use pcl::plugins::resilience::{find_slug_by_container_id, clear_restart};
use crate::DOCKER;
use crate::client::DockerClient;

/// Watch Docker events for plugin containers and sync DB state.
/// Reconnects automatically on error.
pub async fn watch_plugin_events(pool: Option<Pool>) {
    let pool = match pool {
        Some(p) => p,
        None => {
            tracing::warn!("No database configured, skipping Docker event watcher");
            return;
        }
    };

    loop {
        tracing::info!("Starting Docker event watcher");
        if let Err(e) = watch_loop(&pool).await {
            tracing::error!("Docker event stream failed (reconnecting in 5s): {}", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

async fn watch_loop(pool: &Pool) -> Result<(), AppError> {
    use bollard::query_parameters::EventsOptionsBuilder;

    let mut filters = HashMap::new();
    filters.insert("label", vec!["alcedocore.plugin"]);

    let options = EventsOptionsBuilder::default()
        .filters(&filters)
        .build();

    let mut stream = DOCKER.events(Some(options));

    while let Some(event) = stream.next().await {
        let msg = event?;
        handle_event(pool, msg).await;
    }

    tracing::info!("Docker event stream ended");
    Ok(())
}

async fn handle_event(pool: &Pool, msg: bollard::models::EventMessage) {
    let action = match msg.action.as_deref() {
        Some(a) => a,
        None => return,
    };

    let container_id = match msg.actor.as_ref().and_then(|a| a.id.as_deref()) {
        Some(id) => id,
        None => return,
    };

    match action {
        "start" => {
            tracing::info!(container_id = %container_id, "Plugin container started");
            let slug = match find_slug_by_container_id(pool, container_id).await {
                Ok(Some(s)) => s,
                _ => return,
            };

            let _ = sqlx::query(
                "UPDATE plugin_versions SET status = 'running' WHERE container_id = $1"
            )
            .bind(container_id)
            .execute(pool)
            .await;

            let _ = clear_restart(pool, &slug).await;
        }
        "die" => {
            let exit_code = msg.actor.as_ref()
                .and_then(|a| a.attributes.as_ref())
                .and_then(|attrs| attrs.get("exitCode"))
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(-1);

            tracing::info!(container_id = %container_id, exit_code = %exit_code, "Plugin container died");

            let _ = sqlx::query(
                "UPDATE plugin_versions SET status = 'stopped' WHERE container_id = $1"
            )
            .bind(container_id)
            .execute(pool)
            .await;

            let _ = DockerClient.handle_container_exit(pool, container_id, exit_code).await;
        }
        "stop" | "kill" => {
            tracing::info!(container_id = %container_id, action = %action, "Plugin container stopped");

            let _ = sqlx::query(
                "UPDATE plugin_versions SET status = 'stopped' WHERE container_id = $1"
            )
            .bind(container_id)
            .execute(pool)
            .await;
        }
        "destroy" => {
            tracing::info!(container_id = %container_id, "Plugin container destroyed");

            let _ = sqlx::query(
                "UPDATE plugin_versions SET container_id = NULL, status = 'removed' WHERE container_id = $1"
            )
            .bind(container_id)
            .execute(pool)
            .await;
        }
        _ => {}
    }
}
