use std::collections::HashMap;
use std::time::Duration;
use futures_util::StreamExt;

use alcedo_db::db::Pool;
use alcedo_common::AppError;
use alcedo_db::db::resilience::{find_install_id_by_deployment_id, clear_restart};
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

    let deployment_id = match msg.actor.as_ref().and_then(|a| a.id.as_deref()) {
        Some(id) => id,
        None => return,
    };

    match action {
        "start" => {
            tracing::info!(deployment_id = %deployment_id, "Plugin container started");
            let install_id = match find_install_id_by_deployment_id(pool, deployment_id).await {
                Ok(Some(id)) => id,
                _ => return,
            };

            let _ = sqlx::query(
                "UPDATE alcedo_plugin_versions SET status = 'running' WHERE deployment_id = $1"
            )
            .bind(deployment_id)
            .execute(pool)
            .await;

            let _ = clear_restart(pool, install_id).await;
        }
        "die" => {
            let exit_code = msg.actor.as_ref()
                .and_then(|a| a.attributes.as_ref())
                .and_then(|attrs| attrs.get("exitCode"))
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(-1);

            tracing::info!(deployment_id = %deployment_id, exit_code = %exit_code, "Plugin container died");

            let _ = sqlx::query(
                "UPDATE alcedo_plugin_versions SET status = 'stopped' WHERE deployment_id = $1"
            )
            .bind(deployment_id)
            .execute(pool)
            .await;

            let _ = DockerClient.handle_container_exit(pool, deployment_id, exit_code).await;
        }
        "stop" | "kill" => {
            tracing::info!(deployment_id = %deployment_id, action = %action, "Plugin container stopped");

            let _ = sqlx::query(
                "UPDATE alcedo_plugin_versions SET status = 'stopped' WHERE deployment_id = $1"
            )
            .bind(deployment_id)
            .execute(pool)
            .await;
        }
        "destroy" => {
            tracing::info!(deployment_id = %deployment_id, "Plugin container destroyed");

            let _ = sqlx::query(
                "UPDATE alcedo_plugin_versions SET deployment_id = NULL, status = 'removed' WHERE deployment_id = $1"
            )
            .bind(deployment_id)
            .execute(pool)
            .await;
        }
        _ => {}
    }
}
