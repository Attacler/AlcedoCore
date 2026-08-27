use std::collections::HashMap;
use async_trait::async_trait;
use serde_json::Value as JsonValue;
use alcedo_common::AppError;
use alcedo_container::container::ContainerInfo;

use crate::DOCKER;
use crate::client::DockerClient;
use crate::services;

/// Low-level Docker/Swarm-specific operations for the admin API.
/// These are not part of the platform-agnostic PluginPlatform trait.
#[async_trait]
pub trait DockerService: Send + Sync {
    async fn create_network_if_missing(&self, network_name: &str) -> Result<(), AppError>;
    async fn ensure_overlay_network(&self, network_name: &str, hostname: Option<&str>) -> Result<(), AppError>;
    async fn pull_image(&self, image: &str) -> Result<(), AppError>;
    async fn get_container_logs(&self, container_id: &str, tail: usize) -> Result<Vec<String>, AppError>;
    async fn get_container_stats(&self, container_id: &str) -> Result<JsonValue, AppError>;
    async fn resolve_for_exec(&self, container_id: &str) -> Result<String, AppError>;
    async fn get_service_tasks(&self, service_name: &str) -> Result<Vec<JsonValue>, AppError>;
    async fn list_tasks(&self) -> Result<Vec<JsonValue>, AppError>;
    async fn inspect_service(&self, service_name: &str) -> Result<Option<JsonValue>, AppError>;
    async fn delete_service(&self, service_name: &str) -> Result<(), AppError>;
    async fn create_plugin_service(&self, slug: &str, image: &str, env: &HashMap<String, String>, network_name: &str, replicas: i64) -> Result<String, AppError>;
    async fn list_directory_in_image(&self, image_name: &str, dir_path: &str) -> Result<Vec<String>, AppError>;
    async fn list_images(&self) -> Result<Vec<String>, AppError>;
    async fn list_all_containers(&self) -> Result<Vec<ContainerInfo>, AppError>;
    async fn stop_container(&self, container_id: &str, timeout_secs: i64) -> Result<(), AppError>;
    async fn remove_container(&self, container_id: &str, force: bool) -> Result<(), AppError>;
}

pub struct DockerServiceImpl;

#[async_trait]
impl DockerService for DockerServiceImpl {
    async fn ensure_overlay_network(&self, network_name: &str, hostname: Option<&str>) -> Result<(), AppError> {
        use bollard::models::NetworkCreateRequest;
        use bollard::query_parameters::ListNetworksOptions;

        let networks = DOCKER.list_networks(None::<ListNetworksOptions>).await
            .map_err(|e| AppError::DockerError { details: e.to_string() })?;
        let has_overlay = networks.iter().any(|n| {
            n.name.as_deref() == Some(network_name) && n.driver.as_deref() == Some("overlay")
        });
        if !has_overlay {
            DOCKER.create_network(NetworkCreateRequest {
                name: network_name.to_string(),
                driver: Some("overlay".to_string()),
                scope: Some("swarm".to_string()),
                attachable: Some(true),
                ..Default::default()
            }).await.map_err(|e| AppError::DockerError { details: e.to_string() })?;
        }

        if let Some(hostname) = hostname {
            if !hostname.is_empty() {
                use bollard::models::{NetworkConnectRequest, EndpointSettings};
                let _ = DOCKER.disconnect_network(
                    network_name,
                    bollard::models::NetworkDisconnectRequest {
                        container: hostname.to_string(),
                        force: Some(true),
                    },
                ).await;
                if let Err(e) = DOCKER.connect_network(
                    network_name,
                    NetworkConnectRequest {
                        container: hostname.to_string(),
                        endpoint_config: Some(EndpointSettings {
                            aliases: Some(vec!["core".to_string()]),
                            ..Default::default()
                        }),
                    },
                ).await {
                    tracing::warn!("Failed to connect core container to overlay network: {}", e);
                }
            }
        }
        Ok(())
    }

    async fn get_container_logs(&self, container_id: &str, tail: usize) -> Result<Vec<String>, AppError> {
        use bollard::container::LogOutput;
        use bollard::query_parameters::LogsOptions;
        use futures_util::StreamExt;

        let options = LogsOptions {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            ..Default::default()
        };

        let mut log_stream = DOCKER.logs(container_id, Some(options));
        let mut lines = Vec::new();
        while let Some(result) = log_stream.next().await {
            match result {
                Ok(LogOutput::StdOut { message }) | Ok(LogOutput::StdErr { message }) => {
                    let msg = String::from_utf8_lossy(&message).to_string();
                    for line in msg.lines() {
                        if !line.is_empty() {
                            lines.push(line.to_string());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Error reading container logs: {:?}", e);
                }
                _ => {}
            }
        }
        Ok(lines)
    }

    async fn get_container_stats(&self, container_id: &str) -> Result<JsonValue, AppError> {
        use bollard::query_parameters::StatsOptions;
        use futures_util::StreamExt;

        let options = StatsOptions {
            stream: false,
            one_shot: true,
        };

        let mut stats_stream = DOCKER.stats(container_id, Some(options));
        let stats = stats_stream.next().await
            .ok_or_else(|| AppError::Internal("Failed to get container stats".to_string()))?
            .map_err(|e| AppError::DockerError { details: e.to_string() })?;

        serde_json::to_value(&stats)
            .map_err(|e| AppError::Internal(format!("Failed to serialize stats: {}", e)))
    }

    async fn resolve_for_exec(&self, container_id: &str) -> Result<String, AppError> {
        Ok(services::resolve_for_exec(&DOCKER, container_id).await)
    }

    async fn get_service_tasks(&self, service_name: &str) -> Result<Vec<JsonValue>, AppError> {
        let tasks = services::get_service_tasks(&DOCKER, service_name).await?;
        serde_json::to_value(&tasks)
            .map_err(|e| AppError::Internal(format!("Failed to serialize tasks: {}", e)))
            .map(|v| v.as_array().cloned().unwrap_or_default())
    }

    async fn list_tasks(&self) -> Result<Vec<JsonValue>, AppError> {
        use bollard::query_parameters::ListTasksOptions;

        let tasks = DOCKER.list_tasks(None::<ListTasksOptions>).await
            .map_err(|e| AppError::DockerError { details: e.to_string() })?;

        let result: Vec<JsonValue> = tasks.into_iter().map(|t| {
            let mut map = serde_json::Map::new();
            map.insert("id".to_string(), serde_json::json!(t.id));
            if let Some(ref status) = t.status {
                if let Some(ref container_status) = status.container_status {
                    map.insert("container_id".to_string(), serde_json::json!(container_status.container_id));
                }
            }
            serde_json::Value::Object(map)
        }).collect();

        Ok(result)
    }

    async fn inspect_service(&self, service_name: &str) -> Result<Option<JsonValue>, AppError> {
        let service = services::inspect_plugin_service(&DOCKER, service_name).await?;
        Ok(service.map(|s| {
            let mut map = serde_json::Map::new();
            if let Some(ref spec) = s.spec {
                map.insert("name".to_string(), serde_json::json!(spec.name));
            }
            serde_json::Value::Object(map)
        }))
    }

    async fn delete_service(&self, service_name: &str) -> Result<(), AppError> {
        services::remove_plugin_service(&DOCKER, service_name).await
    }

    async fn create_plugin_service(&self, slug: &str, image: &str, env: &HashMap<String, String>, network_name: &str, replicas: i64) -> Result<String, AppError> {
        services::create_plugin_service(&DOCKER, slug, image, env, network_name, None, replicas).await
    }

    async fn list_images(&self) -> Result<Vec<String>, AppError> {
        use bollard::query_parameters::ListImagesOptions;

        let images = DOCKER.list_images(Some(ListImagesOptions::default())).await
            .map_err(|e| AppError::DockerError { details: e.to_string() })?;

        Ok(images.into_iter()
            .flat_map(|i| i.repo_tags)
            .collect())
    }

    async fn list_all_containers(&self) -> Result<Vec<ContainerInfo>, AppError> {
        DockerClient.list_containers().await
    }

    async fn create_network_if_missing(&self, network_name: &str) -> Result<(), AppError> {
        DockerClient.create_network_if_missing(network_name).await
    }

    async fn pull_image(&self, image: &str) -> Result<(), AppError> {
        DockerClient.pull_image(image).await
    }

    async fn list_directory_in_image(&self, image_name: &str, dir_path: &str) -> Result<Vec<String>, AppError> {
        DockerClient.list_directory_in_image(image_name, dir_path).await
    }

    async fn stop_container(&self, container_id: &str, timeout_secs: i64) -> Result<(), AppError> {
        DockerClient.stop_container(container_id, timeout_secs).await
    }

    async fn remove_container(&self, container_id: &str, force: bool) -> Result<(), AppError> {
        DockerClient.remove_container(container_id, force).await
    }
}
