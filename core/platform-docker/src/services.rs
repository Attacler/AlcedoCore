use std::collections::HashMap;
use bollard::Docker;
use bollard::models::{
    ServiceSpec, ServiceSpecMode, ServiceSpecModeReplicated,
    TaskSpec, TaskSpecContainerSpec, NetworkAttachmentConfig,
    EndpointSpec, EndpointSpecModeEnum,
    TaskSpecResources, ResourceObject, Limit,
};
use bollard::query_parameters::{ListServicesOptions, ListTasksOptions, UpdateServiceOptions};
use serde::Serialize;
use alcedo_common::AppError;

#[derive(Debug, Clone)]
pub struct PluginResourceLimits {
    pub cpu_limit: i64,
    pub memory_limit: i64,
}

impl Default for PluginResourceLimits {
    fn default() -> Self {
        Self {
            cpu_limit: 0,
            memory_limit: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInstanceInfo {
    pub task_id: String,
    pub slot: i64,
    pub status: String,
    pub desired_state: String,
    pub container_id: Option<String>,
    pub node_id: Option<String>,
}

pub async fn create_plugin_service(
    docker: &Docker,
    slug: &str,
    image: &str,
    env: &HashMap<String, String>,
    network_name: &str,
    resource_limits: Option<PluginResourceLimits>,
    replicas: i64,
) -> Result<String, AppError> {
    let service_name = format!("plugin_{}", slug);

    let env_vars: Vec<String> = env
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();

    let task_resources = resource_limits.filter(|r| r.cpu_limit > 0 || r.memory_limit > 0).map(|r| TaskSpecResources {
        limits: Some(Limit {
            nano_cpus: if r.cpu_limit > 0 { Some(r.cpu_limit) } else { None },
            memory_bytes: if r.memory_limit > 0 { Some(r.memory_limit) } else { None },
            pids: None,
        }),
        reservations: Some(ResourceObject {
            nano_cpus: if r.cpu_limit > 0 { Some(r.cpu_limit) } else { None },
            memory_bytes: if r.memory_limit > 0 { Some(r.memory_limit) } else { None },
            generic_resources: None,
        }),
        swap_bytes: None,
        memory_swappiness: None,
    });

    let spec = ServiceSpec {
        name: Some(service_name.clone()),
        labels: Some(HashMap::from([("alcedocore.plugin".to_string(), slug.to_string())])),
        task_template: Some(TaskSpec {
            container_spec: Some(TaskSpecContainerSpec {
                image: Some(image.to_string()),
                env: Some(env_vars),
                ..Default::default()
            }),
            networks: if network_name.is_empty() {
                None
            } else {
                Some(vec![NetworkAttachmentConfig {
                    target: Some(network_name.to_string()),
                    ..Default::default()
                }])
            },
            resources: task_resources,
            ..Default::default()
        }),
        mode: Some(ServiceSpecMode {
            replicated: Some(ServiceSpecModeReplicated {
                replicas: Some(replicas as i64),
            }),
            global: None,
            replicated_job: None,
            global_job: None,
        }),
        endpoint_spec: Some(EndpointSpec {
            mode: Some(EndpointSpecModeEnum::VIP),
            ports: None,
        }),
        ..Default::default()
    };

    match docker.create_service(spec, None).await {
        Ok(response) => {
            let service_id = response.id.unwrap_or_else(|| service_name.clone());
            tracing::info!(
                slug = %slug,
                service_name = %service_name,
                service_id = %service_id,
                replicas = %replicas,
                "Created Docker Swarm service"
            );
            Ok(service_id)
        }
        Err(e) => {
            tracing::error!(slug = %slug, error = %e, "Failed to create Docker Swarm service");
            Err(AppError::DockerError { details: e.to_string() })
        }
    }
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
                container_id: t.status.as_ref()
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

pub async fn list_plugin_services(docker: &Docker) -> Result<Vec<String>, AppError> {
    let options = ListServicesOptions {
        ..Default::default()
    };

    match docker.list_services(Some(options)).await {
        Ok(services) => {
            let plugin_services: Vec<String> = services
                .iter()
                .filter_map(|s| s.spec.as_ref())
                .filter_map(|spec| spec.name.clone())
                .filter(|name| name.starts_with("plugin_"))
                .collect();
            Ok(plugin_services)
        }
        Err(e) => {
            tracing::warn!("Failed to list Docker Swarm services: {:?}", e);
            Err(AppError::DockerError { details: e.to_string() })
        }
    }
}

pub async fn inspect_plugin_service(docker: &Docker, service_name: &str) -> Result<Option<bollard::models::Service>, AppError> {
    match docker.inspect_service(service_name, None::<bollard::query_parameters::InspectServiceOptions>).await {
        Ok(service) => Ok(Some(service)),
        Err(bollard::errors::Error::DockerResponseServerError { status_code: 404, .. }) => Ok(None),
        Err(e) => {
            tracing::warn!(service_name = %service_name, error = %e, "Failed to inspect Docker Swarm service");
            Err(AppError::DockerError { details: e.to_string() })
        }
    }
}

pub fn is_swarm_service_name(container_id: &str) -> bool {
    container_id.starts_with("plugin_")
}

pub async fn resolve_service_to_container(docker: &Docker, service_name: &str) -> Result<Option<String>, AppError> {
    let tasks = get_service_tasks(docker, service_name).await?;
    Ok(tasks.into_iter().find(|t| t.status == "running").and_then(|t| t.container_id))
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
