//! Docker Swarm deployment backend.
//!
//! Only compiled with `--features docker`; without it the dependency is not
//! even built and [`super::plugin_platform::MockPlatform`] is always selected.
//!
//! One install id names one deployment, so the service name *is* the routing
//! key: `plugin_{install_id}`. That name is the deployment id persisted on the
//! install row and the DNS name the proxy resolves on the overlay network.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bollard::auth::DockerCredentials;
use bollard::models::{
    NetworkAttachmentConfig, NetworkCreateRequest, ServiceSpec, ServiceSpecMode,
    ServiceSpecModeReplicated, TaskSpec, TaskSpecContainerSpec, TaskSpecRestartPolicy,
    TaskSpecRestartPolicyConditionEnum,
};
use bollard::query_parameters::{CreateImageOptions, ListNetworksOptions};
use bollard::{API_DEFAULT_VERSION, Docker};
use futures::StreamExt;
use serde_json::{Value, json};
use sqlx::{Pool, Postgres, Row};

use crate::services::{config::Config, errors::AlcedoError, plugin_platform::PluginPlatform};

const SERVICE_PREFIX: &str = "plugin_";
const LABEL_PLUGIN: &str = "alcedocore.plugin";
const LABEL_INSTALL_ID: &str = "alcedocore.install_id";

pub struct DockerPlatform {
    docker: Docker,
    pool: Arc<Pool<Postgres>>,
    network: String,
    port: u16,
}

impl DockerPlatform {
    pub async fn connect(
        socket: &str,
        pool: Arc<Pool<Postgres>>,
        config: &Config,
    ) -> Result<Self, AlcedoError> {
        let docker =
            Docker::connect_with_socket(socket, 120, API_DEFAULT_VERSION).map_err(|e| {
                AlcedoError::SystemError(
                    format!("Could not reach the Docker daemon at {}: {}", socket, e),
                    0,
                )
            })?;

        let platform = DockerPlatform {
            docker,
            pool,
            network: config.plugin_network.clone(),
            port: config.plugin_port,
        };
        platform.health_check().await?;
        platform.require_swarm().await?;
        platform.ensure_overlay_network().await?;
        Ok(platform)
    }

    fn service_name(install_id: i64) -> String {
        format!("{}{}", SERVICE_PREFIX, install_id)
    }

    fn is_service_name(deployment_id: &str) -> bool {
        deployment_id.starts_with(SERVICE_PREFIX)
    }

    async fn require_swarm(&self) -> Result<(), AlcedoError> {
        let control = self
            .docker
            .info()
            .await
            .ok()
            .and_then(|info| info.swarm)
            .and_then(|swarm| swarm.control_available)
            .unwrap_or(false);

        if control {
            Ok(())
        } else {
            Err(AlcedoError::SystemError(
                "Docker Swarm is not active on this daemon (control_available = false); \
                 run `docker swarm init --advertise-addr <ip>`"
                    .to_string(),
                0,
            ))
        }
    }

    pub async fn ensure_overlay_network(&self) -> Result<(), AlcedoError> {
        let networks = self
            .docker
            .list_networks(None::<ListNetworksOptions>)
            .await?;

        if let Some(network) = networks
            .iter()
            .find(|n| n.name.as_deref() == Some(self.network.as_str()))
        {
            let driver = network.driver.as_deref().unwrap_or("unknown");
            let scope = network.scope.as_deref().unwrap_or("unknown");

            if driver == "overlay" && scope == "swarm" {
                return Ok(());
            }

            return Err(AlcedoError::SystemError(
                format!(
                    "Network '{}' already exists but is {}/{}, not overlay/swarm. \
                     A swarm service attached to it gets no DNS entry, so /p/{{install_id}} \
                     would never resolve. Point PLUGIN_NETWORK at a free name, or replace it with \
                     `docker network rm {} && docker network create --driver overlay --attachable {}`.",
                    self.network, driver, scope, self.network, self.network
                ),
                0,
            ));
        }

        self.docker
            .create_network(NetworkCreateRequest {
                name: self.network.clone(),
                driver: Some("overlay".to_string()),
                scope: Some("swarm".to_string()),
                attachable: Some(true),
                ..Default::default()
            })
            .await
            .map_err(|e| {
                AlcedoError::SystemError(
                    format!("Could not create overlay network {}: {}", self.network, e),
                    0,
                )
            })?;
        tracing::info!("[DOCKER] Created overlay network {}", self.network);
        Ok(())
    }
    async fn registry_credentials(
        &self,
        slug: &str,
    ) -> Result<Option<DockerCredentials>, AlcedoError> {
        let row = sqlx::query(
            "SELECT r.auth_type, r.username, r.password,
                    COALESCE(NULLIF(r.pull_url, ''), r.url) AS server
             FROM alcedocore.alcedocore_plugins p
             JOIN alcedocore.alcedocore_registries r ON r.id = p.registry_id
             WHERE p.slug = $1",
        )
        .bind(slug)
        .fetch_optional(&*self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let server: Option<String> = row.try_get("server").unwrap_or(None);

        let auth_type: String = row.try_get("auth_type").unwrap_or_default();
        if auth_type.eq_ignore_ascii_case("none") {
            return Ok(Some(DockerCredentials {
                username: None,
                password: None,
                serveraddress: server.map(|s| {
                    s.trim_start_matches("http://")
                        .trim_start_matches("https://")
                        .trim_end_matches('/')
                        .to_string()
                }),
                ..Default::default()
            }));
        }

        let username: Option<String> = row.try_get("username").unwrap_or(None);
        let password: Option<String> = row.try_get("password").unwrap_or(None);
        let (Some(username), Some(password)) = (username, password) else {
            tracing::warn!(
                "[DOCKER] Registry auth_type={} but no username/password for {}; pulling anonymously",
                auth_type,
                slug
            );
            return Ok(None);
        };

        Ok(Some(DockerCredentials {
            username: Some(username),
            password: Some(password),
            serveraddress: server.map(|s| {
                s.trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .trim_end_matches('/')
                    .to_string()
            }),
            ..Default::default()
        }))
    }

    async fn pull_image(
        &self,
        image: &str,
        version: &str,
        credentials: Option<DockerCredentials>,
    ) -> Result<String, AlcedoError> {
        println!("image: {}", image);
        let (from_image, tag) = split_image(image);
        println!("from_image: {} tag: {:?} {}", from_image, tag, version);
        let full_image_name = if let Some(credentials) = &credentials {
            if let Some(server) = &credentials.serveraddress {
                format!("{}/{}", server, from_image)
            } else {
                from_image
            }
        } else {
            from_image
        };
        let full_image_name = format!("{}:{}", full_image_name, version);
        let options = CreateImageOptions {
            from_image: Some(full_image_name.clone()),
            tag: None,
            ..Default::default()
        };
        println!("options: {:?} {:?}", options, credentials);

        let mut stream = self.docker.create_image(Some(options), None, credentials);
        while let Some(item) = stream.next().await {
            item.map_err(|e| {
                AlcedoError::SystemError(format!("Could not pull image {}: {}", image, e), 0)
            })?;
        }
        tracing::info!("[DOCKER] Pulled {}", image);
        Ok(full_image_name)
    }

    async fn create_plugin_service(
        &self,
        name: &str,
        slug: &str,
        install_id: i64,
        image: &str,
        env: HashMap<String, String>,
    ) -> Result<(), AlcedoError> {
        let labels = HashMap::from([
            (LABEL_PLUGIN.to_string(), slug.to_string()),
            (LABEL_INSTALL_ID.to_string(), install_id.to_string()),
        ]);

        let spec = ServiceSpec {
            name: Some(name.to_string()),
            labels: Some(labels.clone()),
            mode: Some(ServiceSpecMode {
                replicated: Some(ServiceSpecModeReplicated { replicas: Some(1) }),
                ..Default::default()
            }),
            task_template: Some(TaskSpec {
                container_spec: Some(TaskSpecContainerSpec {
                    image: Some(image.to_string()),
                    env: Some(env.into_iter().map(|(k, v)| format!("{k}={v}")).collect()),
                    labels: Some(labels),
                    ..Default::default()
                }),
                networks: Some(vec![NetworkAttachmentConfig {
                    target: Some(self.network.clone()),
                    ..Default::default()
                }]),
                restart_policy: Some(TaskSpecRestartPolicy {
                    condition: Some(TaskSpecRestartPolicyConditionEnum::ANY),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.docker.create_service(spec, None).await.map_err(|e| {
            AlcedoError::SystemError(format!("Could not create service {}: {}", name, e), 0)
        })?;
        tracing::info!("[DOCKER] Created Swarm service {} ({})", name, slug);
        Ok(())
    }

    async fn remove_plugin_service(&self, name: &str) -> Result<(), bollard::errors::Error> {
        self.docker.delete_service(name).await
    }

    fn is_not_found(error: &bollard::errors::Error) -> bool {
        matches!(
            error,
            bollard::errors::Error::DockerResponseServerError {
                status_code: 404,
                ..
            }
        )
    }

    /// The install id behind a `plugin_{id}` service name.
    fn install_id_of(deployment_id: &str) -> Option<i64> {
        deployment_id
            .strip_prefix(SERVICE_PREFIX)
            .and_then(|id| id.parse().ok())
    }
}

fn split_image(image: &str) -> (String, Option<String>) {
    match image.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => (name.to_string(), Some(tag.to_string())),
        _ => (image.to_string(), None),
    }
}

#[async_trait]
impl PluginPlatform for DockerPlatform {
    async fn deploy(
        &self,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
        install_id: Option<i64>,
    ) -> Result<String, AlcedoError> {
        let install_id = install_id.ok_or_else(|| {
            AlcedoError::SystemError("Docker deployment requires an install id".to_string(), 0)
        })?;
        let name = Self::service_name(install_id);

        let credentials = self.registry_credentials(slug).await?;
        let image = self.pull_image(image, version, credentials).await?;

        // Redeploy replaces rather than duplicates: the old service goes first
        // so the create cannot collide on the name.
        if let Err(e) = self.remove_plugin_service(&name).await {
            if !Self::is_not_found(&e) {
                tracing::warn!("[DOCKER] Could not remove old service {}: {}", name, e);
            }
        }

        self.create_plugin_service(&name, slug, install_id, &image, env)
            .await?;
        Ok(name)
    }

    async fn get_address(&self, deployment_id: &str) -> Result<Option<String>, AlcedoError> {
        Ok(Some(format!("{}:{}", deployment_id, self.port)))
    }

    fn is_replicated_service(&self, deployment_id: &str) -> bool {
        Self::is_service_name(deployment_id)
    }

    async fn runtime_info(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let name = Self::service_name(install_id);

        let service = match self.docker.inspect_service(&name, None).await {
            Ok(service) => service,
            Err(e) if Self::is_not_found(&e) => {
                return Ok(json!({
                    "install_id": install_id,
                    "deployment_id": null,
                    "status": "not_deployed",
                    "service": Value::Null,
                }));
            }
            Err(e) => {
                return Err(AlcedoError::SystemError(
                    format!("Could not inspect service {}: {}", name, e),
                    0,
                ));
            }
        };

        let spec = service.spec.as_ref();
        let image = spec
            .and_then(|s| s.task_template.as_ref())
            .and_then(|t| t.container_spec.as_ref())
            .and_then(|c| c.image.clone())
            .unwrap_or_default();

        let replicas = spec
            .and_then(|s| s.mode.as_ref())
            .and_then(|m| m.replicated.as_ref())
            .and_then(|r| r.replicas);

        Ok(json!({
            "install_id": install_id,
            "deployment_id": name,
            "status": "running",
            "image": image,
            "replicas": replicas,
            "service": service,
        }))
    }

    async fn instances(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let name = Self::service_name(install_id);

        // Tasks reference the service by id, not by name.
        let service_id = match self.docker.inspect_service(&name, None).await {
            Ok(service) => service.id,
            Err(e) if Self::is_not_found(&e) => None,
            Err(e) => {
                return Err(AlcedoError::SystemError(
                    format!("Could not inspect service {}: {}", name, e),
                    0,
                ));
            }
        };

        let Some(service_id) = service_id else {
            return Ok(json!({ "instances": [] }));
        };

        let tasks = self.docker.list_tasks(None).await.map_err(|e| {
            AlcedoError::SystemError(format!("Could not list tasks for {}: {}", name, e), 0)
        })?;

        let instances: Vec<Value> = tasks
            .iter()
            .filter(|task| task.service_id.as_deref() == Some(service_id.as_str()))
            .map(|task| {
                json!({
                    "id": task.id.clone().unwrap_or_default(),
                    "install_id": install_id,
                    "deployment_id": name,
                    "slot": task.slot.unwrap_or(0),
                    "status": task
                        .status
                        .as_ref()
                        .and_then(|s| s.state.as_ref())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    "desired_state": task
                        .desired_state
                        .as_ref()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    "node_id": task.node_id.clone(),
                })
            })
            .collect();

        // The controller wraps this in the JSend envelope, so it must not carry a
        // second `data` key of its own.
        Ok(json!({ "instances": instances }))
    }

    async fn list_deployments(&self) -> Result<Vec<Value>, AlcedoError> {
        let services =
            self.docker.list_services(None).await.map_err(|e| {
                AlcedoError::SystemError(format!("Could not list services: {}", e), 0)
            })?;

        Ok(services
            .iter()
            .filter(|service| {
                service
                    .spec
                    .as_ref()
                    .and_then(|s| s.labels.as_ref())
                    .is_some_and(|labels| labels.contains_key(LABEL_PLUGIN))
            })
            .map(|service| {
                let spec = service.spec.as_ref();
                json!({
                    "deployment_id": spec.and_then(|s| s.name.clone()),
                    "install_id": spec
                        .and_then(|s| s.labels.as_ref())
                        .and_then(|l| l.get(LABEL_INSTALL_ID))
                        .and_then(|v| v.parse::<i64>().ok()),
                    "slug": spec
                        .and_then(|s| s.labels.as_ref())
                        .and_then(|l| l.get(LABEL_PLUGIN))
                        .cloned(),
                    "image": spec
                        .and_then(|s| s.task_template.as_ref())
                        .and_then(|t| t.container_spec.as_ref())
                        .and_then(|c| c.image.clone()),
                    "replicas": spec
                        .and_then(|s| s.mode.as_ref())
                        .and_then(|m| m.replicated.as_ref())
                        .and_then(|r| r.replicas),
                })
            })
            .collect())
    }

    async fn health_check(&self) -> Result<(), AlcedoError> {
        self.docker.ping().await.map(|_| ()).map_err(|e| {
            AlcedoError::SystemError(format!("Docker daemon is unreachable: {}", e), 0)
        })
    }

    async fn ensure_absent(&self, install_id: i64) -> Result<(), AlcedoError> {
        let name = Self::service_name(install_id);
        match self.docker.delete_service(&name).await {
            Ok(()) => {
                tracing::info!("[DOCKER] Removed Swarm service {}", name);
                Ok(())
            }
            // Teardown is best-effort: the row is already gone, so a missing
            // service is the desired end state, not a failure.
            Err(e) if Self::is_not_found(&e) => Ok(()),
            Err(e) => {
                tracing::warn!("[DOCKER] Could not remove service {}: {}", name, e);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_are_install_keyed() {
        assert_eq!(DockerPlatform::service_name(7), "plugin_7");
        assert_eq!(DockerPlatform::install_id_of("plugin_7"), Some(7));
        assert_eq!(DockerPlatform::install_id_of("mock-7"), None);
    }

    #[test]
    fn recognizes_its_own_service_names() {
        assert!(DockerPlatform::is_service_name("plugin_7"));
        assert!(!DockerPlatform::is_service_name("mock-7"));
    }

    #[test]
    fn splits_image_reference_from_tag() {
        assert_eq!(
            split_image("localhost:5000/hello-world:1.0.0"),
            (
                "localhost:5000/hello-world".to_string(),
                Some("1.0.0".to_string())
            )
        );
        // No tag, and a registry port that must not be mistaken for one.
        assert_eq!(
            split_image("localhost:5000/hello-world"),
            ("localhost:5000/hello-world".to_string(), None)
        );
        assert_eq!(split_image("nginx"), ("nginx".to_string(), None));
    }
}
