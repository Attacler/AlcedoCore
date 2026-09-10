use alcedo_db::queries::Registry;
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Platform-agnostic types
// ---------------------------------------------------------------------------

/// Opaque identifier for a deployed plugin instance.
/// Docker → container ID, K8s → pod/service name, Railway → service ID.
pub type DeploymentId = String;

/// Events emitted by the platform runtime for lifecycle tracking.
#[derive(Debug, Clone)]
pub enum DeploymentEvent {
    Started {
        slug: String,
        id: DeploymentId,
    },
    Stopped {
        slug: String,
        id: DeploymentId,
        exit_code: i64,
    },
    HealthChanged {
        slug: String,
        healthy: bool,
    },
}

/// Summary info about a running deployment.
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub status: String,
}

/// Detailed info about a specific deployment.
#[derive(Debug, Clone)]
pub struct ContainerDetails {
    pub id: String,
    pub name: String,
    pub state: String,
    pub created: String,
    pub image: String,
    pub network_mode: Option<String>,
}

/// Info about a single instance (replica/pod/task) within a deployment.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub id: String,
    pub status: String,
    pub pod_name: String,
    pub container_id: Option<String>,
}

/// CPU/memory stats snapshot for an instance.
#[derive(Debug, Clone, Serialize)]
pub struct ContainerStatsSnapshot {
    pub timestamp: String,
    pub cpu_percent: f64,
    pub memory_usage_bytes: i64,
}

/// Info about a container image.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub id: String,
    pub tags: Vec<String>,
    pub size: i64,
    pub created: String,
}

// ---------------------------------------------------------------------------
// ContainerRuntime trait — platform-agnostic container operations
// ---------------------------------------------------------------------------

/// Platform-agnostic container runtime operations.
/// Each deployment backend (Docker/Swarm, K8s, Railway) implements this trait.
#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    async fn pull_image(&self, image: &str, registry: &Registry) -> Result<String, AppError>;
    async fn create_container(
        &self,
        registry: &Registry,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
        network_mode: Option<&str>,
    ) -> Result<String, AppError>;
    async fn start_container(&self, container_id: &str) -> Result<(), AppError>;
    async fn stop_container(&self, container_id: &str, timeout_secs: i64) -> Result<(), AppError>;
    async fn remove_container(&self, container_id: &str, force: bool) -> Result<(), AppError>;
    async fn list_containers(&self) -> Result<Vec<ContainerInfo>, AppError>;
    async fn inspect_container(&self, container_id: &str) -> Result<ContainerDetails, AppError>;
    async fn get_container_ip(
        &self,
        container_id: &str,
        network_name: &str,
    ) -> Result<Option<String>, AppError>;
    async fn restart_container(&self, container_id: &str) -> Result<(), AppError>;
    async fn inspect_image(&self, image_name: &str) -> Result<ImageInfo, AppError>;
    async fn get_file_from_image(
        &self,
        registry: &Registry,
        image_name: &str,
        file_path: &str,
    ) -> Result<String, AppError>;
    async fn get_file_from_container(
        &self,
        container_id: &str,
        path: &str,
    ) -> Result<Vec<u8>, AppError>;
    async fn list_directory_in_container(
        &self,
        container_id: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError>;
    async fn list_directory_recursive_in_container(
        &self,
        container_id: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError>;
    async fn copy_directory_from_container(
        &self,
        container_id: &str,
        container_path: &str,
        host_dest: &str,
    ) -> Result<(), AppError>;
    async fn copy_directory_from_image(
        &self,
        registry: &Registry,
        image_name: &str,
        container_path: &str,
        host_dest: &str,
    ) -> Result<(), AppError>;
    async fn connect_container_to_network(
        &self,
        container_id: &str,
        network_name: &str,
        alias: Option<&str>,
    ) -> Result<(), AppError>;
}

// ---------------------------------------------------------------------------
// DockerService trait — low-level Docker operations for the admin API
// ---------------------------------------------------------------------------

/// Low-level Docker operations needed by the admin/plugin API handlers.
// ---------------------------------------------------------------------------
// PluginPlatform trait — the single abstraction for all deployment targets
// ---------------------------------------------------------------------------

/// Platform-agnostic interface for deploying and managing plugins.
///
/// Each deployment backend (Docker/Swarm, K8s, Railway) implements this trait.
/// The shared core never references platform-specific types like container IDs
/// or pod names — it operates entirely through `DeploymentId` and this trait.
#[async_trait]
pub trait PluginPlatform: Send + Sync {
    /// Ensure the container image is available (pull if needed).
    async fn ensure_image(&self, registry: &Registry, image: &str) -> Result<String, AppError>;

    /// Deploy a plugin version. Handles image pull, container/pod creation,
    /// network attachment, and startup. Returns a `DeploymentId`.
    async fn deploy(
        &self,
        registry: &Registry,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
    ) -> Result<DeploymentId, AppError>;

    /// Remove a deployment (stop + destroy).
    async fn remove(&self, id: &DeploymentId) -> Result<(), AppError>;

    /// Restart a deployment.
    async fn restart(&self, id: &DeploymentId) -> Result<(), AppError>;

    /// Get the network address for proxy routing.
    /// Returns `None` if the deployment isn't reachable.
    async fn get_address(&self, id: &DeploymentId) -> Result<Option<String>, AppError>;

    /// Check whether a deployment ID refers to a replicated service
    /// (Swarm service, K8s deployment) vs a single instance.
    async fn is_replicated_service(&self, id: &DeploymentId) -> bool;

    /// Scale a replicated service to the given replica count.
    async fn scale(&self, id: &DeploymentId, replicas: u32) -> Result<(), AppError>;

    /// Read a file from a container image (for manifest/migration extraction).
    async fn read_file_from_image(
        &self,
        registry: &Registry,
        image: &str,
        path: &str,
    ) -> Result<String, AppError>;

    /// Extract a directory from a container image to the host filesystem.
    async fn extract_from_image(
        &self,
        registry: &Registry,
        image: &str,
        src: &str,
        dest: &str,
    ) -> Result<(), AppError>;

    /// Read a file from a running deployment (for debugging / admin UI).
    async fn read_file(&self, id: &DeploymentId, path: &str) -> Result<Vec<u8>, AppError>;

    /// List files in a directory within a running deployment.
    async fn list_directory(&self, id: &DeploymentId, path: &str) -> Result<Vec<String>, AppError>;

    /// Check whether the platform backend is healthy.
    async fn health_check(&self) -> Result<(), AppError>;

    /// Subscribe to deployment lifecycle events.
    /// The platform should emit events into the sender until cancelled.
    async fn watch_events(&self, tx: mpsc::Sender<DeploymentEvent>) -> Result<(), AppError>;

    /// List all running deployments.
    async fn list_deployments(&self) -> Result<Vec<ContainerInfo>, AppError>;

    /// Inspect a single deployment.
    async fn inspect(&self, id: &DeploymentId) -> Result<ContainerDetails, AppError>;

    /// List running instances (replicas/pods/tasks) for a deployment.
    async fn list_instances(&self, id: &DeploymentId) -> Result<Vec<InstanceInfo>, AppError>;

    /// Get logs for a specific instance within a deployment.
    async fn get_instance_logs(
        &self,
        id: &DeploymentId,
        instance_id: &str,
        tail: usize,
    ) -> Result<String, AppError>;

    /// Get CPU/memory stats for a specific instance within a deployment.
    async fn get_instance_stats(
        &self,
        id: &DeploymentId,
        instance_id: &str,
    ) -> Result<ContainerStatsSnapshot, AppError>;

    /// List files in a directory within a container image (for preview/migrations).
    async fn list_directory_in_image(
        &self,
        registry: &Registry,
        image: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError>;

    /// Inspect a container image.
    async fn inspect_image(&self, image: &str) -> Result<ImageInfo, AppError>;

    /// Platform-accessible URL that plugins can use to call back to the core API.
    /// Called during deploy to inject `CORE_URL` into plugin environments.
    fn core_url(&self) -> String {
        "http://core:8080".to_string()
    }
}
