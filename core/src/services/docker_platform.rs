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
    ContainerCreateBody, NetworkAttachmentConfig, NetworkCreateRequest, ServiceSpec,
    ServiceSpecMode, ServiceSpecModeReplicated, TaskSpec, TaskSpecContainerSpec,
    TaskSpecRestartPolicy, TaskSpecRestartPolicyConditionEnum,
};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, DownloadFromContainerOptions, ListImagesOptions,
    ListNetworksOptions, LogsOptionsBuilder, RemoveContainerOptions, StatsOptionsBuilder,
    UpdateServiceOptions,
};
use bollard::{API_DEFAULT_VERSION, Docker};
use futures::StreamExt;
use serde_json::{Value, json};
use sqlx::{Pool, Postgres, Row};

use crate::services::{
    config::Config,
    encryption,
    errors::AlcedoError,
    plugin_platform::{PluginPlatform, ResourceLimits},
    registry_client::authority_of,
};
use crate::utils::{image_reference, split_image};

const SERVICE_PREFIX: &str = "plugin_";

fn unpack_stripping_root(bytes: &[u8], dest: &std::path::Path) -> Result<(), std::io::Error> {
    let root = common_root_component(bytes);
    let mut archive = tar::Archive::new(std::io::Cursor::new(bytes));

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let stripped = strip_component(&path, root.as_deref()).unwrap_or_else(|| path.clone());

        if stripped.is_absolute()
            || stripped
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            tracing::warn!(
                "[DOCKER] Skipping archive entry that escapes the destination: {:?}",
                path
            );
            continue;
        }

        // Belt and braces on an already-rejected input.
        let target = dest.join(&stripped);
        if !target.starts_with(dest) {
            tracing::warn!(
                "[DOCKER] Skipping archive entry outside destination: {:?}",
                path
            );
            continue;
        }

        match entry.header().entry_type() {
            tar::EntryType::Directory => {
                std::fs::create_dir_all(&target)?;
            }
            tar::EntryType::Regular => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut file = std::fs::File::create(&target)?;
                std::io::copy(&mut entry, &mut file)?;
            }
            other => {
                tracing::debug!("[DOCKER] Skipping {:?} entry {:?}", other, path);
            }
        }
    }

    Ok(())
}

fn common_root_component(bytes: &[u8]) -> Option<String> {
    let mut archive = tar::Archive::new(std::io::Cursor::new(bytes));
    let entries = archive.entries().ok()?;

    let mut root: Option<String> = None;
    for entry in entries.flatten() {
        let is_dir = entry.header().entry_type() == tar::EntryType::Directory;
        let path = entry.path().ok()?.into_owned();
        let mut components = path.components();

        let Some(first_component) = components.next() else {
            continue;
        };
        let first = first_component.as_os_str().to_string_lossy().to_string();

        if components.next().is_none() {
            if !is_dir {
                return None;
            }
            root.get_or_insert(first);
            continue;
        }

        match &root {
            Some(existing) if *existing == first => {}
            _ => {
                if root.is_some() {
                    return None;
                }
                root = Some(first);
            }
        }
    }

    root
}

fn strip_component(path: &std::path::Path, root: Option<&str>) -> Option<std::path::PathBuf> {
    let mut components = path.components();
    let first = components.next()?;
    if first.as_os_str().to_string_lossy() == root.unwrap_or("\u{0}") {
        Some(components.as_path().to_path_buf())
    } else {
        None
    }
}

async fn apply_spec_update<F>(
    docker: &Docker,
    install_id: i64,
    action: &str,
    mutate: F,
) -> Result<(), AlcedoError>
where
    F: Fn(&mut ServiceSpec),
{
    const ATTEMPTS: u32 = 5;
    let name = format!("{}{}", SERVICE_PREFIX, install_id);

    let mut last_error = String::new();
    for attempt in 1..=ATTEMPTS {
        let service = docker.inspect_service(&name, None).await.map_err(|e| {
            if DockerPlatform::is_not_found(&e) {
                AlcedoError::NotFound(format!("Plugin is not deployed: {}", name), 0)
            } else {
                AlcedoError::SystemError(format!("Could not inspect service {}: {}", name, e), 0)
            }
        })?;

        let version = service
            .version
            .as_ref()
            .and_then(|v| v.index)
            .unwrap_or_default() as i32;

        let mut spec = service
            .spec
            .clone()
            .ok_or_else(|| AlcedoError::SystemError(format!("Service {} has no spec", name), 0))?;
        mutate(&mut spec);

        match docker
            .update_service(
                &name,
                spec,
                UpdateServiceOptions {
                    version,
                    registry_auth_from: "spec".to_string(),
                    ..Default::default()
                },
                None,
            )
            .await
        {
            Ok(_) => {
                tracing::info!("[DOCKER] {} applied to service {}", action, name);
                return Ok(());
            }
            Err(e) => {
                last_error = e.to_string();
                let busy = last_error.contains("update out of sequence");
                if !busy || attempt == ATTEMPTS {
                    return Err(AlcedoError::SystemError(
                        format!("Could not {} service {}: {}", action, name, last_error),
                        0,
                    ));
                }
                tracing::info!(
                    "[DOCKER] Service {} still converging, retrying {}/{}",
                    name,
                    attempt,
                    ATTEMPTS
                );
                tokio::time::sleep(std::time::Duration::from_millis(500 * (1 << (attempt - 1))))
                    .await;
            }
        }
    }

    Err(AlcedoError::SystemError(
        format!("Could not {} service {}: {}", action, name, last_error),
        0,
    ))
}

/// The manifest sits at the image's `WORKDIR`, which every system and sample
/// plugin sets to `/app`.
const MANIFEST_CANDIDATES: &[&str] = &["/app/manifest.json"];

fn split_reference(reference: &str) -> Option<(String, String)> {
    let (name, tag) = reference.rsplit_once(':')?;
    // `<none>` is Docker's placeholder for an untagged layer, not a name.
    let placeholder = |part: &str| part.is_empty() || part == "none" || part == "<none>";
    if placeholder(name) || placeholder(tag) {
        return None;
    }
    Some((name.to_string(), tag.to_string()))
}

fn is_path_absent(error: &bollard::errors::Error) -> bool {
    match error {
        bollard::errors::Error::DockerResponseServerError {
            status_code,
            message,
            ..
        } => {
            *status_code == 404
                || message.contains("Could not find the file")
                || message.contains("no such file or directory")
        }
        _ => false,
    }
}

async fn discard_container(docker: &Docker, id: &str) {
    if let Err(e) = docker
        .remove_container(
            id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
    {
        tracing::warn!(
            "[DOCKER] Could not remove extraction container {}: {}",
            id,
            e
        );
    }
}
const LABEL_PLUGIN: &str = "alcedocore.plugin";
const LABEL_INSTALL_ID: &str = "alcedocore.install_id";
const PROXY_BASIC_USER: &str = "alcedo";

pub struct DockerPlatform {
    docker: Docker,
    pool: Arc<Pool<Postgres>>,
    network: String,
    port: u16,
    host_authority: Option<String>,
    proxy_token: Option<String>,
}

impl DockerPlatform {
    /// The tasks backing an install's service, or `None` when the service does
    /// not exist.
    async fn service_tasks(
        &self,
        install_id: i64,
    ) -> Result<Option<Vec<bollard::models::Task>>, AlcedoError> {
        let name = Self::service_name(install_id);
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
            return Ok(None);
        };

        let tasks = self.docker.list_tasks(None).await.map_err(|e| {
            AlcedoError::SystemError(format!("Could not list tasks for {}: {}", name, e), 0)
        })?;
        Ok(Some(
            tasks
                .into_iter()
                .filter(|task| task.service_id.as_deref() == Some(service_id.as_str()))
                .collect(),
        ))
    }

    /// One task of an install, by its own id.
    ///
    /// Resolved through the service rather than by listing tasks directly, so a
    /// task id from a different service cannot be read through this install.
    async fn find_task(
        &self,
        install_id: i64,
        task_id: &str,
    ) -> Result<bollard::models::Task, AlcedoError> {
        let tasks = self.service_tasks(install_id).await?.unwrap_or_default();
        tasks
            .into_iter()
            .find(|task| task.id.as_deref() == Some(task_id))
            .ok_or_else(|| {
                AlcedoError::NotFound(
                    format!("Task {} is not part of install {}", task_id, install_id),
                    0,
                )
            })
    }

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
            host_authority: authority_of(&config.host_url).map(str::to_string),
            proxy_token: config.root_system_developer_token.clone(),
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
            "SELECT r.auth_type, r.username, r.password, r.url, r.pull_url
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

        let url: Option<String> = row.try_get("url").unwrap_or(None);
        let pull_url: Option<String> = row.try_get("pull_url").unwrap_or(None);

        let server = pull_url
            .as_deref()
            .and_then(server_address)
            .or_else(|| url.as_deref().and_then(server_address));

        let via_proxy = server
            .as_deref()
            .zip(self.host_authority.as_deref())
            .is_some_and(|(server, host)| server.eq_ignore_ascii_case(host));

        if via_proxy && self.proxy_token.is_none() {
            tracing::warn!(
                "[DOCKER] {} pulls via this instance's registry proxy but \
                 ROOT_SYSTEM_DEVELOPER_TOKEN is unset; the pull will be rejected as unauthenticated",
                slug
            );
        }
        let proxy_password = match (&via_proxy, self.proxy_token.as_deref()) {
            (true, Some(token)) if !token.is_empty() => Some(token.to_string()),
            _ => None,
        };
        let proxy_username = proxy_password
            .as_ref()
            .map(|_| PROXY_BASIC_USER.to_string());

        if let Some(server) = &server {
            tracing::info!(
                "[DOCKER] Pulling {} from registry {}{}",
                slug,
                server,
                if via_proxy { " (via proxy)" } else { "" }
            );
        }

        let auth_type: String = row.try_get("auth_type").unwrap_or_default();
        if auth_type.eq_ignore_ascii_case("none") {
            return Ok(Some(DockerCredentials {
                username: proxy_username,
                password: proxy_password,
                serveraddress: server,
                ..Default::default()
            }));
        }

        let username: Option<String> = row.try_get("username").unwrap_or(None);
        let stored: Option<String> = row.try_get("password").unwrap_or(None);

        let password: Option<String> = stored.map(|stored| {
            match encryption::decrypt(&stored) {
                Ok(plaintext) => plaintext,
                // Tolerate rows written before encryption was introduced.
                Err(_) => {
                    tracing::warn!(
                        "[DOCKER] Stored registry password is not decryptable; using it verbatim"
                    );
                    stored
                }
            }
        });
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
            serveraddress: server,
            ..Default::default()
        }))
    }

    async fn pull_image(
        &self,
        image: &str,
        version: &str,
        credentials: Option<DockerCredentials>,
    ) -> Result<String, AlcedoError> {
        let (from_image, _tag) = split_image(image);

        let server = credentials
            .as_ref()
            .and_then(|c| c.serveraddress.as_deref())
            .filter(|_| !is_registry_qualified(&from_image));

        let full_image_name = match server {
            Some(server) => format!("{}/{}", server, from_image),
            None => from_image,
        };

        let full_image_name = format!("{}:{}", full_image_name, version);
        let options = CreateImageOptions {
            from_image: Some(full_image_name.clone()),
            tag: None,
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(options), None, credentials);
        while let Some(item) = stream.next().await {
            item.map_err(|e| {
                AlcedoError::SystemError(format!("Could not pull image {}: {}", image, e), 0)
            })?;
        }
        tracing::info!("[DOCKER] Pulled {}", full_image_name);
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

fn is_registry_qualified(image: &str) -> bool {
    match image.split_once('/') {
        Some((head, _)) => head == "localhost" || head.contains('.') || head.contains(':'),
        None => false,
    }
}

fn server_address(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let without_scheme = trimmed
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    // Drop any path so `host:port/prefix` still yields just the authority.
    let host = without_scheme.split('/').next().unwrap_or_default();
    (!host.is_empty()).then(|| host.to_string())
}

#[cfg(test)]
mod registry_address_tests {
    use super::server_address;

    #[test]
    fn server_address_drops_scheme_and_path() {
        assert_eq!(
            server_address("http://registry:5000").as_deref(),
            Some("registry:5000")
        );
        assert_eq!(
            server_address("https://reg.example.com/").as_deref(),
            Some("reg.example.com")
        );
        assert_eq!(
            server_address("registry:5000").as_deref(),
            Some("registry:5000")
        );
        assert_eq!(
            server_address("http://localhost:8080").as_deref(),
            Some("localhost:8080")
        );
        assert_eq!(server_address(""), None);
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

        // Image facts the admin UI's Versions tab renders. A missing image is
        // normal rather than fatal — Swarm may be running a tag this daemon no
        // longer holds — so a failed inspect yields nulls, not an error.
        let image_info = if image.is_empty() {
            None
        } else {
            self.docker.inspect_image(&image).await.ok()
        };
        let image_id = image_info
            .as_ref()
            .and_then(|i| i.id.as_deref())
            .map(|id| id.strip_prefix("sha256:").unwrap_or(id).to_string());
        let image_size = image_info.as_ref().and_then(|i| i.size);
        let image_tags = image_info
            .as_ref()
            .and_then(|i| i.repo_tags.clone())
            .unwrap_or_default();

        // Container state, aggregated over the service's tasks: the service
        // existing only means it was created, not that anything is up.
        let service_id = service.id.as_deref();
        let deployment_state = match self.docker.list_tasks(None).await {
            Ok(tasks) => {
                let states: Vec<String> = tasks
                    .iter()
                    .filter(|t| service_id.is_some() && t.service_id.as_deref() == service_id)
                    .filter_map(|t| t.status.as_ref()?.state.as_ref().map(|s| s.to_string()))
                    .collect();
                if states.iter().any(|s| s == "running") {
                    "running"
                } else if states.iter().any(|s| s == "failed") {
                    "failed"
                } else {
                    "stopped"
                }
            }
            Err(_) => "unknown",
        };

        Ok(json!({
            "install_id": install_id,
            "deployment_id": name,
            "status": "running",
            "deployment_state": deployment_state,
            "image": image,
            "image_id": image_id,
            "size": image_size,
            "tags": image_tags,
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
            // Swarm keeps every task a service ever ran until the service is
            // updated, so the task list is history as well as the live set.
            // Terminal tasks have no container to show, measure, or read logs
            // from — they are past deployments, not instances.
            .filter(|task| {
                !matches!(
                    task.status.as_ref().and_then(|s| s.state.as_ref()),
                    Some(
                        bollard::models::TaskState::COMPLETE
                            | bollard::models::TaskState::SHUTDOWN
                            | bollard::models::TaskState::FAILED
                            | bollard::models::TaskState::REJECTED
                            | bollard::models::TaskState::REMOVE
                            | bollard::models::TaskState::ORPHANED
                    )
                )
            })
            .map(|task| {
                json!({
                    "task_id": task.id.clone().unwrap_or_default(),
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

    /// Pulls the image if needed, then reads its manifest.
    ///
    /// A pull failure is not fatal — the image is often already local, and a
    /// preview is a question rather than a deployment. A manifest that is
    /// missing or unparseable is likewise reported as absent rather than as a
    /// hard error, so the endpoint can still report the slug it inferred.
    async fn preview_manifest(&self, image: &str) -> Result<Option<Value>, AlcedoError> {
        let (_, tag) = split_image(image);
        let version = tag.unwrap_or_else(|| "latest".to_string());
        let reference = image_reference(image, &version);

        let slug = image
            .rsplit('/')
            .next()
            .unwrap_or(image)
            .split(':')
            .next()
            .unwrap_or("")
            .to_string();

        let credentials = self.registry_credentials(&slug).await.unwrap_or(None);
        if let Err(e) = self.pull_image(image, &version, credentials).await {
            tracing::info!(
                "[DOCKER] Preview pull of {} failed ({}); trying what is local",
                reference,
                e
            );
        }

        let staging = std::env::temp_dir().join(format!("alcedo-preview-{}", uuid::Uuid::new_v4()));

        let mut manifest = None;
        for candidate in MANIFEST_CANDIDATES {
            match self
                .extract_from_image(&reference, &version, candidate, &staging)
                .await
            {
                Ok(true) => {
                    // A single file has no wrapping directory to strip, so it
                    // lands under its own name.
                    let name = candidate.rsplit('/').next().unwrap_or("manifest.json");
                    manifest = std::fs::read_to_string(staging.join(name)).ok();
                    if manifest.is_some() {
                        break;
                    }
                }
                Ok(false) => continue,
                Err(e) => {
                    tracing::warn!("[DOCKER] Preview could not read {}: {}", candidate, e);
                    break;
                }
            }
        }

        let _ = std::fs::remove_dir_all(&staging);

        Ok(manifest.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()))
    }

    async fn local_images(&self) -> Result<Vec<Value>, AlcedoError> {
        let images = self
            .docker
            .list_images(Some(ListImagesOptions {
                all: true,
                ..Default::default()
            }))
            .await
            .map_err(|e| {
                AlcedoError::SystemError(format!("Could not list local images: {}", e), 0)
            })?;

        let mut out = Vec::new();
        for image in images {
            for reference in image.repo_tags {
                if let Some((name, tag)) = split_reference(&reference) {
                    out.push(json!({ "name": name, "tag": tag }));
                }
            }
        }

        out.sort_by(|a, b| {
            let an = a["name"].as_str().unwrap_or_default();
            let bn = b["name"].as_str().unwrap_or_default();
            let at = a["tag"].as_str().unwrap_or_default();
            let bt = b["tag"].as_str().unwrap_or_default();
            an.cmp(bn).then(at.cmp(bt))
        });
        out.dedup_by(|a, b| a == b);
        Ok(out)
    }

    async fn extract_from_image(
        &self,
        image: &str,
        _version: &str,
        src: &str,
        dest: &std::path::Path,
    ) -> Result<bool, AlcedoError> {
        let container_name = format!("alcedo-extract-{}", uuid::Uuid::new_v4());

        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(container_name.clone()),
                    ..Default::default()
                }),
                ContainerCreateBody {
                    image: Some(image.to_string()),
                    // A plugin's real entrypoint is irrelevant here and would
                    // fail without its config, so the container is created to
                    // exit immediately and never actually runs.
                    entrypoint: Some(vec!["/bin/true".to_string()]),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| {
                AlcedoError::SystemError(
                    format!("Could not create extraction container for {}: {}", image, e),
                    0,
                )
            })?;

        let mut stream = self.docker.download_from_container(
            &created.id,
            Some(DownloadFromContainerOptions {
                path: src.to_string(),
            }),
        );

        let mut archive_bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => archive_bytes.extend_from_slice(&bytes),
                Err(e) => {
                    discard_container(&self.docker, &created.id).await;
                    if is_path_absent(&e) {
                        tracing::info!("[DOCKER] {} not in {} (nothing extracted)", src, image);
                        return Ok(false);
                    }
                    return Err(AlcedoError::SystemError(
                        format!("Could not read {} from {}: {}", src, image, e),
                        0,
                    ));
                }
            }
        }

        discard_container(&self.docker, &created.id).await;

        if archive_bytes.is_empty() {
            tracing::info!(
                "[DOCKER] {} not present in {} (nothing extracted)",
                src,
                image
            );
            std::fs::create_dir_all(dest).map_err(|e| {
                AlcedoError::SystemError(format!("Could not create {}: {}", dest.display(), e), 0)
            })?;
            return Ok(false);
        }

        std::fs::create_dir_all(dest).map_err(|e| {
            AlcedoError::SystemError(format!("Could not create {}: {}", dest.display(), e), 0)
        })?;

        unpack_stripping_root(&archive_bytes, dest).map_err(|e| {
            AlcedoError::SystemError(format!("Could not unpack {} from {}: {}", src, image, e), 0)
        })?;

        tracing::info!(
            "[DOCKER] Extracted {} from {} to {}",
            src,
            image,
            dest.display()
        );
        Ok(true)
    }

    async fn restart(&self, install_id: i64) -> Result<(), AlcedoError> {
        apply_spec_update(&self.docker, install_id, "restart", |spec| {
            spec.task_template = spec.task_template.clone().map(|mut task| {
                task.force_update = Some(task.force_update.unwrap_or(0) + 1);
                task
            });
        })
        .await
    }

    async fn scale(
        &self,
        install_id: i64,
        replicas: u32,
        limits: Option<ResourceLimits>,
    ) -> Result<(), AlcedoError> {
        let name = Self::service_name(install_id);

        let service = self
            .docker
            .inspect_service(&name, None)
            .await
            .map_err(|e| {
                if Self::is_not_found(&e) {
                    AlcedoError::NotFound(format!("Plugin is not deployed: {}", name), 0)
                } else {
                    AlcedoError::SystemError(
                        format!("Could not inspect service {}: {}", name, e),
                        0,
                    )
                }
            })?;

        if service
            .spec
            .as_ref()
            .and_then(|s| s.mode.as_ref())
            .and_then(|m| m.replicated.as_ref())
            .is_none()
        {
            return Err(AlcedoError::InvalidInput(
                format!("{} runs in global mode and cannot be scaled", name),
                0,
            ));
        }

        apply_spec_update(&self.docker, install_id, "scale", |spec| {
            spec.mode = Some(ServiceSpecMode {
                replicated: Some(ServiceSpecModeReplicated {
                    replicas: Some(i64::from(replicas)),
                }),
                ..Default::default()
            });

            if let Some(limits) = limits.filter(|l| !l.is_empty()) {
                let task_template = spec.task_template.get_or_insert_with(Default::default);
                let resources = task_template.resources.get_or_insert_with(Default::default);
                let applied = resources.limits.get_or_insert_with(Default::default);
                if let Some(nano_cpus) = limits.cpu_limit {
                    applied.nano_cpus = Some(nano_cpus);
                }
                if let Some(memory_bytes) = limits.memory_limit {
                    applied.memory_bytes = Some(memory_bytes);
                }
            }
        })
        .await?;

        tracing::info!("[DOCKER] Scaled service {} to {} replicas", name, replicas);
        Ok(())
    }

    async fn instance_stats(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError> {
        let task = self.find_task(install_id, task_id).await?;
        let container_id = task
            .status
            .as_ref()
            .and_then(|s| s.container_status.as_ref())
            .and_then(|c| c.container_id.as_deref())
            .ok_or_else(|| {
                // A task that has not been scheduled yet has no container to
                // measure, which is a normal state rather than a failure.
                AlcedoError::InvalidInput(format!("Task {} has no container yet", task_id), 0)
            })?;

        let mut stream = self.docker.stats(
            container_id,
            Some(StatsOptionsBuilder::default().stream(true).build()),
        );

        let mut stats = None;
        for _ in 0..3 {
            let Some(sample) = stream.next().await else {
                break;
            };
            let sample = sample
                .map_err(|e| AlcedoError::SystemError(format!("Could not read stats: {e}"), 0))?;
            let interval_sample = sample
                .precpu_stats
                .as_ref()
                .and_then(|p| p.system_cpu_usage)
                .is_some_and(|v| v > 0);
            stats = Some(sample);
            if interval_sample {
                break;
            }
        }

        let stats = stats.ok_or_else(|| {
            AlcedoError::SystemError("Container stats returned no sample".to_string(), 0)
        })?;

        Ok(json!({
            "task_id": task_id,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "cpu_percent": cpu_percent(&stats),
            "memory_usage_bytes": memory_usage(&stats),
        }))
    }

    async fn instance_detail(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError> {
        let name = Self::service_name(install_id);
        let task = self.find_task(install_id, task_id).await?;

        let state = |value: Option<&bollard::models::TaskState>| {
            value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        };
        let image = task
            .spec
            .as_ref()
            .and_then(|s| s.container_spec.as_ref())
            .and_then(|c| c.image.clone())
            .unwrap_or_default();
        let created = task.created_at.map(|d| d.to_string()).unwrap_or_default();

        Ok(json!({
            "task_id": task_id,
            "slot": task.slot.unwrap_or(0),
            "status": state(task.status.as_ref().and_then(|s| s.state.as_ref())),
            "desired_state": task.desired_state.as_ref().map(|s| s.to_string()).unwrap_or_else(|| "unknown".to_string()),
            "deployment_id": name,
            "node_id": task.node_id.clone(),
            // The drawer's "deployment" block is the container the task runs;
            // under Swarm that is the service plus this task's image.
            "deployment": {
                "name": name,
                "state": state(task.status.as_ref().and_then(|s| s.state.as_ref())),
                "image": image,
                "created": created,
                "network_mode": Value::Null,
            },
        }))
    }

    async fn instance_logs(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError> {
        let task = self.find_task(install_id, task_id).await?;
        let container_id = task
            .status
            .as_ref()
            .and_then(|s| s.container_status.as_ref())
            .and_then(|c| c.container_id.as_deref())
            .ok_or_else(|| {
                AlcedoError::InvalidInput(format!("Task {} has no container yet", task_id), 0)
            })?;

        let options = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .tail("200")
            .build();

        let mut logs = self.docker.logs(container_id, Some(options));
        let mut lines: Vec<String> = Vec::new();
        while let Some(chunk) = logs.next().await {
            let chunk = chunk.map_err(|e| {
                AlcedoError::SystemError(
                    format!("Could not read logs of task {}: {}", task_id, e),
                    0,
                )
            })?;

            let bytes = chunk.into_bytes();
            let text = String::from_utf8_lossy(&bytes);
            for line in text.lines() {
                if !line.is_empty() {
                    lines.push(line.to_string());
                }
            }
        }

        Ok(json!({
            "task_id": task_id,
            "logs": lines,
        }))
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

    async fn watch_events(&self) {
        self.watch_events_loop().await;
    }
}

impl DockerPlatform {
    async fn watch_events_loop(&self) {
        use bollard::query_parameters::EventsOptionsBuilder;
        use std::collections::HashMap as StdHashMap;

        loop {
            tracing::info!("[RECOVERY] Starting Docker event watcher");

            let mut filters = StdHashMap::new();
            filters.insert("label".to_string(), vec![LABEL_PLUGIN.to_string()]);
            let options = EventsOptionsBuilder::default().filters(&filters).build();

            let mut stream = self.docker.events(Some(options));

            while let Some(event) = stream.next().await {
                let msg = match event {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!(
                            "[RECOVERY] Docker event stream error (reconnecting in 5s): {}",
                            e
                        );
                        break;
                    }
                };
                self.handle_plugin_event(msg).await;
            }

            tracing::info!("[RECOVERY] Docker event stream ended, reconnecting in 5s");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    async fn handle_plugin_event(&self, msg: bollard::models::EventMessage) {
        use crate::services::plugin_recovery;

        let action = match msg.action.as_deref() {
            Some(a) => a,
            None => return,
        };

        // The install id is in the event actor's attributes (service label).
        let install_id: Option<i64> = msg
            .actor
            .as_ref()
            .and_then(|a| a.attributes.as_ref())
            .and_then(|attrs| attrs.get(LABEL_INSTALL_ID))
            .and_then(|v| v.parse::<i64>().ok());

        let Some(install_id) = install_id else {
            return;
        };

        match action {
            "start" => {
                tracing::info!(install_id = %install_id, "[RECOVERY] Plugin container started");
                if let Err(e) = plugin_recovery::clear_restart(&self.pool, install_id).await {
                    tracing::warn!(
                        "[RECOVERY] Could not clear restart state for {}: {}",
                        install_id,
                        e
                    );
                }
            }
            "die" => {
                let exit_code = msg
                    .actor
                    .as_ref()
                    .and_then(|a| a.attributes.as_ref())
                    .and_then(|attrs| attrs.get("exitCode"))
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(-1);

                tracing::info!(
                    install_id = %install_id,
                    exit_code = %exit_code,
                    "[RECOVERY] Plugin container died"
                );

                // Only restart on non-zero exit (crash), not on clean stop.
                if exit_code == 0 {
                    return;
                }

                let max_attempts = plugin_recovery::DEFAULT_MAX_RESTART_ATTEMPTS;
                match plugin_recovery::should_restart(&self.pool, install_id, max_attempts).await {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::warn!(
                            install_id = %install_id,
                            "[RECOVERY] Plugin exceeded {} restart attempts, entering failed state",
                            max_attempts
                        );
                        return;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[RECOVERY] Could not check restart eligibility for {}: {}",
                            install_id,
                            e
                        );
                        return;
                    }
                }

                // Record the attempt and compute backoff.
                let error_msg = format!("Container exited with code {}", exit_code);
                if let Err(e) =
                    plugin_recovery::record_restart(&self.pool, install_id, Some(&error_msg)).await
                {
                    tracing::warn!(
                        "[RECOVERY] Could not record restart for {}: {}",
                        install_id,
                        e
                    );
                }

                // Look up current restart count for backoff delay.
                let count = plugin_recovery::get_restart_count(&self.pool, install_id)
                    .await
                    .unwrap_or(0);
                let backoff = plugin_recovery::get_backoff_delay(count as u8);

                tracing::info!(
                    install_id = %install_id,
                    backoff_secs = %backoff.as_secs(),
                    "[RECOVERY] Restarting plugin after backoff"
                );

                tokio::time::sleep(backoff).await;

                // Restart: scale to 1 if replicated, otherwise force-update.
                let result = if self.is_replicated_service(&format!("plugin_{}", install_id)) {
                    self.scale(install_id, 1, None).await
                } else {
                    self.restart(install_id).await
                };

                if let Err(e) = result {
                    tracing::error!("[RECOVERY] Failed to restart plugin {}: {}", install_id, e);
                } else {
                    tracing::info!(install_id = %install_id, "[RECOVERY] Plugin restarted");
                }
            }
            _ => {}
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
    fn detects_an_already_qualified_image() {
        // Already carries a registry: prepending another one would produce
        // `localhost:8080/localhost:5000/hello-world`.
        assert!(is_registry_qualified("localhost:5000/hello-world"));
        assert!(is_registry_qualified("ghcr.io/org/plugin"));
        assert!(is_registry_qualified(
            "registry.example.com:5000/team/plugin"
        ));
        assert!(is_registry_qualified("localhost/plugin"));
    }

    #[test]
    fn a_bare_repository_is_not_qualified() {
        assert!(!is_registry_qualified("hello-world"));
        assert!(!is_registry_qualified("nginx"));
        // `org/plugin` is a namespace, not a registry host.
        assert!(!is_registry_qualified("org/plugin"));
    }

    /// Builds an in-memory tar the way `download_from_container` returns one.
    fn tar_of(paths: &[&str]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for path in paths {
            let data = b"x";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, &data[..])
                .expect("append to test tar");
        }
        builder.into_inner().expect("finish test tar")
    }

    /// Like [`tar_of`] but bypasses `append_data`'s path sanitisation, so a
    /// genuinely hostile name lands in the archive instead of being rejected
    /// before the code under test ever sees it.
    fn tar_of_unchecked(names: &[&[u8]]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for name in names {
            let data = b"x";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            let name = &name[..name.len().min(100)];
            header.as_gnu_mut().expect("gnu header").name[..name.len()].copy_from_slice(name);
            header.set_cksum();
            builder.append(&header, &data[..]).expect("append");
        }
        builder.into_inner().expect("finish test tar")
    }

    /// Probes which `update_service` payload shape Swarm actually accepts.
    ///
    /// Ignored by default: needs a live Swarm with a deployed plugin service.
    /// Kept because the working shape is not documented anywhere and was found
    /// empirically — replaying `inspect_service`'s spec fails with "update out
    /// of sequence", and a minimal spec fails to serialise.
    #[tokio::test]
    #[ignore = "needs a live Swarm; run with --ignored"]
    async fn probe_update_service_payload_shapes() {
        let socket = std::env::var("DOCKER_SOCKET").unwrap_or_default();
        let socket = if socket.is_empty() {
            "/var/run/docker.sock".to_string()
        } else {
            socket
        };
        let config = crate::services::config::get_config();
        let pool = crate::services::postgres::pool::setup_pool(&config)
            .await
            .expect("pool");
        let platform = DockerPlatform::connect(&socket, pool, &config)
            .await
            .expect("connect");

        let name = std::env::var("PROBE_SERVICE").expect("PROBE_SERVICE=plugin_83");
        let docker = &platform.docker;

        let inspected = docker.inspect_service(&name, None).await.expect("inspect");
        let version_index = inspected.version.as_ref().and_then(|v| v.index);

        // 1. Replay the full inspected spec.
        let full = inspected.spec.clone().unwrap_or_default();
        println!(
            "1 full/no-version: {:?}",
            docker
                .update_service(&name, full.clone(), UpdateServiceOptions::default(), None)
                .await
        );

        // 2. Replay the full spec, but send Swarm's current version as the CAS token.
        let opts = UpdateServiceOptions {
            version: version_index.unwrap_or_default() as i32,
            ..Default::default()
        };
        println!(
            "2 full/with-version({version_index:?}): {:?}",
            docker.update_service(&name, full.clone(), opts, None).await
        );

        // 3. Minimal spec (what `docker service scale` sends).
        let minimal = ServiceSpec {
            mode: Some(ServiceSpecMode {
                replicated: Some(ServiceSpecModeReplicated { replicas: Some(1) }),
                ..Default::default()
            }),
            ..Default::default()
        };
        println!(
            "3 minimal: {}",
            serde_json::to_string(&minimal).unwrap_or_default()
        );
        println!(
            "3 minimal result: {:?}",
            docker
                .update_service(
                    &name,
                    minimal.clone(),
                    UpdateServiceOptions::default(),
                    None
                )
                .await
        );

        // 4. Minimal spec plus registryAuthFrom, as the CLI sends it.
        println!(
            "4 minimal+authFrom: {:?}",
            docker
                .update_service(
                    &name,
                    minimal,
                    UpdateServiceOptions {
                        registry_auth_from: "spec".to_string(),
                        ..Default::default()
                    },
                    None,
                )
                .await
        );
    }

    #[test]
    fn splitting_a_reference_keeps_registry_ports_with_the_name() {
        assert_eq!(
            split_reference("hello-world:1.0.0"),
            Some(("hello-world".to_string(), "1.0.0".to_string()))
        );
        // The colon in the port must not be taken for the tag separator.
        assert_eq!(
            split_reference("localhost:5000/hello-world:1.0.0"),
            Some((
                "localhost:5000/hello-world".to_string(),
                "1.0.0".to_string()
            ))
        );
    }

    #[test]
    fn untagged_layers_are_not_offered_as_deployable_images() {
        assert_eq!(split_reference("<none>:<none>"), None);
        assert_eq!(split_reference("hello-world:<none>"), None);
        assert_eq!(split_reference("hello-world:"), None);
        assert_eq!(split_reference(":1.0.0"), None);
        // No colon at all is not a reference we can deploy from.
        assert_eq!(split_reference("hello-world"), None);
    }

    #[test]
    fn strips_the_wrapping_directory_docker_adds() {
        let bytes = tar_of(&["docs/index.md", "docs/api.md", "docs/js/app.js"]);

        let dir = std::env::temp_dir().join(format!("alcedo-extract-{}", uuid::Uuid::new_v4()));
        unpack_stripping_root(&bytes, &dir).expect("unpack");

        assert!(
            dir.join("index.md").exists(),
            "leading 'docs/' must be gone"
        );
        assert!(dir.join("api.md").exists());
        assert!(dir.join("js/app.js").exists());
        assert!(!dir.join("docs").exists(), "must not nest a second time");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Builds the archive shape `download_from_container("/docs")` actually
    /// returns: explicit directory entries, not just file paths. This is the case
    /// a path-only depth check gets wrong.
    fn tar_of_real_shape(files: &[(&str, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for dir in ["docs", "docs/js"] {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("{dir}/"), std::io::empty())
                .expect("append dir");
        }
        for (path, body) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, body.as_bytes())
                .expect("append file");
        }
        builder.into_inner().expect("finish test tar")
    }

    #[test]
    fn strips_the_wrapper_even_with_directory_entries_present() {
        let bytes = tar_of_real_shape(&[
            ("docs/index.md", "# Index"),
            ("docs/js/app.js", "console.log(1)"),
        ]);

        let dir = std::env::temp_dir().join(format!("alcedo-extract-{}", uuid::Uuid::new_v4()));
        unpack_stripping_root(&bytes, &dir).expect("unpack");

        assert!(dir.join("index.md").exists(), "wrapper must be stripped");
        assert!(dir.join("js/app.js").exists(), "nested paths must survive");
        assert!(!dir.join("docs").exists(), "must not nest twice");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leaves_an_archive_that_is_already_at_the_root() {
        // Two different top-level entries means there is no wrapping directory
        // to strip, and stripping anything would corrupt the layout.
        let bytes = tar_of(&["index.md", "api.md"]);

        let dir = std::env::temp_dir().join(format!("alcedo-extract-{}", uuid::Uuid::new_v4()));
        unpack_stripping_root(&bytes, &dir).expect("unpack");

        assert!(dir.join("index.md").exists());
        assert!(dir.join("api.md").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keeps_a_single_file_at_the_archive_root() {
        let bytes = tar_of(&["manifest.json"]);
        let dir = std::env::temp_dir().join(format!("alcedo-extract-{}", uuid::Uuid::new_v4()));
        unpack_stripping_root(&bytes, &dir).expect("unpack");

        // Stripping the only component would leave nothing to extract.
        assert!(dir.join("manifest.json").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extraction_cannot_escape_the_destination() {
        // Both entries share a leading `..`, so `common_root_component` reports
        // `..` as the wrapping directory and strips it. That must not turn
        // `../../b.md` into an escaping write.
        let bytes = tar_of_unchecked(&[b"../a.md", b"../../b.md"]);
        let parent = std::env::temp_dir().join(format!("alcedo-extract-{}", uuid::Uuid::new_v4()));
        let dir = parent.join("out");
        std::fs::create_dir_all(&dir).unwrap();

        let outcome = unpack_stripping_root(&bytes, &dir);

        assert!(
            !parent.join("b.md").exists(),
            "archive wrote outside its destination"
        );
        assert!(
            !dir.join("b.md").exists(),
            "entry containing '..' must be skipped, not relocated"
        );
        let _ = outcome;

        std::fs::remove_dir_all(&parent).ok();
    }
}

/// CPU usage as a percentage of the host, the way `docker stats` reports it.
///
/// This is an interval percentage: streamed stats carry `cpu_stats` and
/// `precpu_stats` spanning the ~1s interval between samples, and the deltas
/// between them give the rate. One-shot samples carry no `precpu_stats`, so
/// they yield 0.0.
fn cpu_percent(stats: &bollard::models::ContainerStatsResponse) -> f64 {
    let Some(cpu) = stats.cpu_stats.as_ref() else {
        return 0.0;
    };
    let Some(cpu_usage) = cpu.cpu_usage.as_ref() else {
        return 0.0;
    };
    let Some(precpu) = stats.precpu_stats.as_ref() else {
        return 0.0;
    };
    let Some(precpu_usage) = precpu.cpu_usage.as_ref() else {
        return 0.0;
    };
    let online = cpu
        .online_cpus
        .map(i64::from)
        .or_else(|| {
            cpu_usage
                .percpu_usage
                .as_ref()
                .filter(|u| !u.is_empty())
                .map(|u| i64::try_from(u.len()).unwrap_or(1))
        })
        .unwrap_or(1) as f64;

    let Some(precpu_system) = precpu.system_cpu_usage else {
        return 0.0;
    };
    if precpu_system <= 0 {
        return 0.0;
    }
    let cpu_delta =
        cpu_usage.total_usage.unwrap_or(0) as i64 - precpu_usage.total_usage.unwrap_or(0) as i64;
    let system_delta = cpu.system_cpu_usage.unwrap_or(0) - precpu_system;

    if cpu_delta < 0 || system_delta <= 0 {
        return 0.0;
    }
    (cpu_delta as f64 / system_delta as f64) * online * 100.0
}

/// Memory in use, with the page cache subtracted as `docker stats` does.
///
/// Reporting the raw cgroup total makes a container look like it is holding
/// far more than it is, since reclaimable cache counts towards the limit.
fn memory_usage(stats: &bollard::models::ContainerStatsResponse) -> u64 {
    let Some(memory) = stats.memory_stats.as_ref() else {
        return 0;
    };
    let usage = memory.usage.unwrap_or(0);
    let cache = memory
        .stats
        .as_ref()
        .and_then(|s| {
            s.get("inactive_file")
                .or_else(|| s.get("total_inactive_file"))
                .copied()
        })
        .unwrap_or(0);
    usage.saturating_sub(cache)
}
