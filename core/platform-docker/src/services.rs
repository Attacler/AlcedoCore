use bollard::Docker;
use bollard::models::{
    ServiceSpecMode, ServiceSpecModeReplicated,
};
use bollard::query_parameters::{ListTasksOptions, UpdateServiceOptions};
use serde::Serialize;
use alcedo_common::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInstanceInfo {
    pub task_id: String,
    pub slot: i64,
    pub status: String,
    pub desired_state: String,
    pub deployment_id: Option<String>,
    pub node_id: Option<String>,
}

pub async fn ensure_overlay_network(
    docker: &Docker,
    network_name: &str,
    hostname: Option<&str>,
) -> Result<(), AppError> {
    use bollard::models::NetworkCreateRequest;
    use bollard::query_parameters::ListNetworksOptions;

    let networks = docker
        .list_networks(None::<ListNetworksOptions>)
        .await
        .map_err(|e| AppError::DockerError {
            details: e.to_string(),
        })?;
    let has_overlay = networks.iter().any(|n| {
        n.name.as_deref() == Some(network_name) && n.driver.as_deref() == Some("overlay")
    });
    if !has_overlay {
        docker
            .create_network(NetworkCreateRequest {
                name: network_name.to_string(),
                driver: Some("overlay".to_string()),
                scope: Some("swarm".to_string()),
                attachable: Some(true),
                ..Default::default()
            })
            .await
            .map_err(|e| AppError::DockerError {
                details: e.to_string(),
            })?;
    }

    if let Some(hostname) = hostname {
        if !hostname.is_empty() {
            use bollard::models::{EndpointSettings, NetworkConnectRequest};
            let _ = docker
                .disconnect_network(
                    network_name,
                    bollard::models::NetworkDisconnectRequest {
                        container: hostname.to_string(),
                        force: Some(true),
                    },
                )
                .await;
            if let Err(e) = docker
                .connect_network(
                    network_name,
                    NetworkConnectRequest {
                        container: hostname.to_string(),
                        endpoint_config: Some(EndpointSettings {
                            aliases: Some(vec!["core".to_string()]),
                            ..Default::default()
                        }),
                    },
                )
                .await
            {
                tracing::warn!("Failed to connect core container to overlay network: {}", e);
            }
        }
    }
    Ok(())
}

pub async fn restart_plugin_service(docker: &Docker, service_name: &str) -> Result<(), AppError> {
    let service = match docker.inspect_service(service_name, None::<bollard::query_parameters::InspectServiceOptions>).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(service_name = %service_name, error = %e, "Failed to inspect service for restart");
            return Err(AppError::DockerError { details: e.to_string() });
        }
    };

    // Read current replica count from the service spec
    let replicas = service.spec.as_ref()
        .and_then(|s| s.mode.as_ref())
        .and_then(|m| m.replicated.as_ref())
        .and_then(|r| r.replicas)
        .unwrap_or(1);

    tracing::info!(service_name = %service_name, replicas = %replicas, "Restarting Swarm service by scaling down and up");

    // Scale down to 0 to stop all tasks
    scale_plugin_service(docker, service_name, 0).await?;

    // Small delay to let Swarm settle
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Scale back to original count - Swarm creates fresh tasks
    scale_plugin_service(docker, service_name, replicas).await?;

    tracing::info!(service_name = %service_name, replicas = %replicas, "Swarm service restarted");
    Ok(())
}

pub async fn scale_plugin_service(docker: &Docker, service_name: &str, replicas: i64) -> Result<(), AppError> {
    let service = match docker.inspect_service(service_name, None::<bollard::query_parameters::InspectServiceOptions>).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(service_name = %service_name, error = %e, "Failed to inspect service for scaling");
            return Err(AppError::DockerError { details: e.to_string() });
        }
    };

    let mut spec = service.spec.unwrap_or_default();
    if let Some(ref mut mode) = spec.mode {
        if let Some(ref mut replicated) = mode.replicated {
            replicated.replicas = Some(replicas as i64);
        } else {
            mode.replicated = Some(ServiceSpecModeReplicated {
                replicas: Some(replicas as i64),
            });
        }
    } else {
        spec.mode = Some(ServiceSpecMode {
            replicated: Some(ServiceSpecModeReplicated {
                replicas: Some(replicas as i64),
            }),
            global: None,
            replicated_job: None,
            global_job: None,
        });
    }

    let version_index = service.version
        .and_then(|v| v.index)
        .unwrap_or(0) as i32;
    let options = UpdateServiceOptions {
        version: version_index,
        ..Default::default()
    };

    match docker.update_service(service_name, spec, options, None).await {
        Ok(_) => {
            tracing::info!(service_name = %service_name, replicas = %replicas, "Scaled Docker Swarm service");
            Ok(())
        }
        Err(e) => {
            tracing::error!(service_name = %service_name, replicas = %replicas, error = %e, "Failed to scale Docker Swarm service");
            Err(AppError::DockerError { details: e.to_string() })
        }
    }
}

pub async fn get_service_tasks(docker: &Docker, service_name: &str) -> Result<Vec<ServiceInstanceInfo>, AppError> {
    // First, get the service to resolve its ID
    let service_id = match docker.inspect_service(service_name, None::<bollard::query_parameters::InspectServiceOptions>).await {
        Ok(s) => s.id.unwrap_or_default(),
        Err(bollard::errors::Error::DockerResponseServerError { status_code: 404, .. }) => {
            tracing::info!(service_name = %service_name, "Service not found — returning empty instance list");
            return Ok(Vec::new());
        }
        Err(e) => {
            tracing::warn!(service_name = %service_name, error = %e, "Failed to inspect service for tasks");
            return Err(AppError::DockerError { details: e.to_string() });
        }
    };

    match docker.list_tasks(None::<ListTasksOptions>).await {
        Ok(tasks) => {
            let instances: Vec<ServiceInstanceInfo> = tasks.iter()
                .filter(|t| t.service_id.as_ref().map_or(false, |sid| *sid == service_id))
                .map(|t| ServiceInstanceInfo {
                task_id: t.id.clone().unwrap_or_default(),
                slot: t.slot.unwrap_or(0),
                status: t.status.as_ref()
                    .and_then(|s| s.state.clone())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                desired_state: t.desired_state.clone()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                deployment_id: t.status.as_ref()
                    .and_then(|s| s.container_status.as_ref())
                    .and_then(|cs| cs.container_id.clone()),
                node_id: t.node_id.clone(),
            }).collect();
            Ok(instances)
        }
        Err(e) => {
            tracing::warn!(service_name = %service_name, error = %e, "Failed to list service tasks");
            Err(AppError::DockerError { details: e.to_string() })
        }
    }
}

pub async fn remove_plugin_service(docker: &Docker, service_name: &str) -> Result<(), AppError> {
    match docker.delete_service(service_name).await {
        Ok(_) => {
            tracing::info!(service_name = %service_name, "Removed Docker Swarm service");
            Ok(())
        }
        Err(e) => {
            tracing::warn!(service_name = %service_name, error = %e, "Failed to remove Docker Swarm service");
            Err(AppError::DockerError { details: e.to_string() })
        }
    }
}

pub fn is_swarm_service_name(deployment_id: &str) -> bool {
    deployment_id.starts_with("plugin_")
}

pub async fn resolve_service_to_container(docker: &Docker, service_name: &str) -> Result<Option<String>, AppError> {
    let tasks = get_service_tasks(docker, service_name).await?;
    Ok(tasks.into_iter().find(|t| t.status == "running").and_then(|t| t.deployment_id))
}

/// Resolve a potentially Swarm service name to a real container ID for use with
/// Docker container APIs (exec, inspect, stats, cp, etc.).
///
/// - If `id` is a Swarm service name (`plugin_<slug>`), resolves it to a running
///   task's container ID. Falls back to the original value if resolution fails.
/// - If `id` is already a real container ID, returns it as-is.
pub async fn resolve_for_exec(docker: &Docker, id: &str) -> String {
    if is_swarm_service_name(id) {
        match resolve_service_to_container(docker, id).await {
            Ok(Some(cid)) => cid,
            _ => id.to_string(),
        }
    } else {
        id.to_string()
    }
}
