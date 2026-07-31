use crate::DOCKER;
use bollard::models::{
    ContainerCreateBody, HostConfig, Mount, NetworkConnectRequest, NetworkCreateRequest,
    PortBinding, RestartPolicy, RestartPolicyNameEnum,
};
use bollard::query_parameters::{
    CreateContainerOptions, DownloadFromContainerOptions, ListContainersOptions, ListImagesOptions,
    ListNetworksOptions, RemoveContainerOptions, StartContainerOptions, StopContainerOptions,
};
use bollard_stubs::query_parameters::CreateImageOptions;
use futures_util::StreamExt;
use pcl::db::Pool;
use pcl::plugins::resilience::{
    find_slug_by_container_id, get_backoff_delay, record_restart, should_restart,
};
use pcl::AppError;
use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use pcl::container::{ContainerDetails, ContainerInfo, ImageInfo};

#[derive(Clone)]
pub struct DockerClient;

impl DockerClient {
    pub async fn pull_image(&self, image: &str) -> Result<(), AppError> {
        // Check if the image already exists locally first.
        let mut filters = HashMap::new();
        filters.insert("reference".to_string(), vec![image.to_string()]);
        let list_options = ListImagesOptions {
            filters: Some(filters),
            ..Default::default()
        };
        match DOCKER.list_images(Some(list_options)).await {
            Ok(existing) if !existing.is_empty() => {
                tracing::info!(
                    "[DOCKER] Image already exists locally, skipping pull: {}",
                    image
                );
                return Ok(());
            }
            Ok(_) => {
                tracing::info!("[DOCKER] Image not found locally, pulling: {}", image);
            }
            Err(e) => {
                tracing::warn!(
                    "[DOCKER] Failed to list images, falling back to pull: {} ({:?})",
                    image,
                    e
                );
            }
        }

        let options = CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };

        let mut stream = DOCKER.create_image(Some(options), None, None);
        while let Some(result) = stream.next().await {
            if let Err(e) = result {
                return Err(AppError::DockerError {
                    details: e.to_string(),
                });
            }
        }
        Ok(())
    }

    pub async fn create_container(
        &self,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
        network_mode: Option<&str>,
    ) -> Result<String, AppError> {
        self.pull_image(image).await?;
        let name = format!("{}-{}", slug, version);

        let env_vars: Vec<String> = env
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        let host_config = HostConfig {
            network_mode: network_mode.map(|nm| nm.to_string()),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),

            ..Default::default()
        };

        let exposed_ports = vec!["8000/tcp".to_string()];

        let config = ContainerCreateBody {
            image: Some(image.to_string()),
            env: Some(env_vars),
            host_config: Some(host_config),
            exposed_ports: Some(exposed_ports),
            // entrypoint: Some(vec!["/bin/true".to_string()]),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: Some(name),
            ..Default::default()
        };
        // println!("config: {:?}", options);
        let response = DOCKER.create_container(Some(options), config).await?;
        // println!("{:?}", response);
        Ok(response.id)
    }

    pub async fn start_container(&self, container_id: &str) -> Result<(), AppError> {
        DOCKER
            .start_container(container_id, None::<StartContainerOptions>)
            .await?;
        Ok(())
    }

    pub async fn stop_container(
        &self,
        container_id: &str,
        _timeout_secs: i64,
    ) -> Result<(), AppError> {
        DOCKER
            .stop_container(container_id, None::<StopContainerOptions>)
            .await?;
        Ok(())
    }

    pub async fn remove_container(&self, container_id: &str, force: bool) -> Result<(), AppError> {
        let options = RemoveContainerOptions {
            force,
            ..Default::default()
        };
        let remove = DOCKER.remove_container(container_id, Some(options)).await?;
        println!("Resp: {:?}:{:?}", remove, container_id);
        Ok(())
    }

    pub async fn list_containers(&self) -> Result<Vec<ContainerInfo>, AppError> {
        let options = ListContainersOptions {
            all: true,
            ..Default::default()
        };

        let containers = DOCKER.list_containers(Some(options)).await?;
        Ok(containers
            .into_iter()
            .map(|c| ContainerInfo {
                id: c.id.unwrap_or_default(),
                name: c
                    .names
                    .unwrap_or_default()
                    .first()
                    .cloned()
                    .unwrap_or_default(),
                status: c.status.unwrap_or_default(),
            })
            .collect())
    }

    pub async fn inspect_container(
        &self,
        container_id: &str,
    ) -> Result<ContainerDetails, AppError> {
        let info = DOCKER.inspect_container(container_id, None).await?;
        let state = info
            .state
            .and_then(|s| s.status)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let network_mode = info
            .host_config
            .map(|hc| hc.network_mode.clone())
            .unwrap_or_default();
        Ok(ContainerDetails {
            id: info.id.unwrap_or_default(),
            name: info.name.unwrap_or_else(|| "".to_string()),
            state,
            created: info
                .created
                .map(|c| c.to_string())
                .unwrap_or_else(|| "0".to_string()),
            image: info.config.and_then(|c| c.image).unwrap_or_default(),
            network_mode,
        })
    }

    pub async fn get_container_ip(
        &self,
        container_id: &str,
        network_name: &str,
    ) -> Result<Option<String>, AppError> {
        let info = DOCKER.inspect_container(container_id, None).await?;
        if let Some(networks) = info.network_settings.map(|ns| ns.networks).flatten() {
            if let Some(network_settings) = networks.get(network_name) {
                match &network_settings.ip_address {
                    Some(ip) if !ip.is_empty() => return Ok(Some(ip.clone())),
                    _ => return Ok(None),
                }
            }
        }
        Ok(None)
    }

    pub async fn is_host_network_mode(&self, container_id: &str) -> Result<bool, AppError> {
        let info = DOCKER.inspect_container(container_id, None).await?;
        let network_mode = info
            .host_config
            .map(|hc| hc.network_mode.clone())
            .unwrap_or_default();
        Ok(network_mode == Some("host".to_string()) || network_mode == Some("HOST".to_string()))
    }

    pub async fn restart_container(&self, container_id: &str) -> Result<(), AppError> {
        self.stop_container(container_id, 60).await?;
        self.start_container(container_id).await
    }

    pub async fn restart_container_with_backoff(
        &self,
        db: &Pool,
        container_id: &str,
        backoff_delay: Duration,
    ) -> Result<(), AppError> {
        let slug = match find_slug_by_container_id(db, container_id).await? {
            Some(s) => s,
            None => {
                tracing::warn!(container_id = %container_id, "Cannot find slug for container, skipping restart");
                return Ok(());
            }
        };

        let max_attempts = pcl::config::AppConfig::from_env()
            .map(|c| c.max_restart_attempts as i32)
            .unwrap_or(3);

        if !should_restart(db, &slug, max_attempts).await? {
            tracing::warn!(container_id = %container_id, "Container exceeded restart attempts, entering failed state");
            return Err(AppError::Internal(format!(
                "Container {} entered failed state after {} restart attempts",
                container_id, max_attempts
            )));
        }

        record_restart(db, &slug, None).await?;
        tokio::time::sleep(backoff_delay).await;
        self.start_container(container_id).await
    }

    pub async fn handle_container_exit(
        &self,
        db: &Pool,
        container_id: &str,
        exit_code: i32,
    ) -> Result<(), AppError> {
        if exit_code != 0 {
            tracing::info!(container_id = %container_id, exit_code = %exit_code, "Container exited with non-zero code, checking restart eligibility");

            let slug = match find_slug_by_container_id(db, container_id).await? {
                Some(s) => s,
                None => {
                    tracing::warn!(container_id = %container_id, "Cannot find slug for container, skipping restart check");
                    return Ok(());
                }
            };

            let max_attempts = pcl::config::AppConfig::from_env()
                .map(|c| c.max_restart_attempts as i32)
                .unwrap_or(3);

            if should_restart(db, &slug, max_attempts).await? {
                let recovery = pcl::db::queries::PluginRecovery::find_by_slug(db, &slug).await?;
                let backoff =
                    get_backoff_delay(recovery.map(|r| r.restart_count as u8).unwrap_or(0));
                self.restart_container_with_backoff(db, container_id, backoff)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn connect_container_to_network(
        &self,
        container_id: &str,
        network_name: &str,
    ) -> Result<(), AppError> {
        let config = NetworkConnectRequest {
            container: container_id.to_string(),
            endpoint_config: Some(bollard_stubs::models::EndpointSettings::default()),
        };
        DOCKER.connect_network(network_name, config).await?;
        Ok(())
    }

    pub async fn list_networks(&self) -> Result<Vec<String>, AppError> {
        let options = ListNetworksOptions {
            ..Default::default()
        };
        let networks = DOCKER.list_networks(Some(options)).await?;
        Ok(networks
            .into_iter()
            .map(|n| n.name.unwrap_or_default())
            .collect())
    }

    pub async fn create_network_if_missing(&self, network_name: &str) -> Result<(), AppError> {
        let networks = self.list_networks().await?;
        if !networks.contains(&network_name.to_string()) {
            let config = NetworkCreateRequest {
                name: network_name.to_string(),
                driver: Some("bridge".to_string()),
                ..Default::default()
            };
            DOCKER.create_network(config).await?;
        }
        Ok(())
    }

    pub async fn health_check(&self) -> Result<(), AppError> {
        DOCKER.ping().await?;
        Ok(())
    }

    pub async fn inspect_image(&self, image_name: &str) -> Result<ImageInfo, AppError> {
        let info = DOCKER.inspect_image(image_name).await?;
        let tags = info.repo_tags.unwrap_or_default();
        let size = info.size.unwrap_or(0);
        let created = info
            .created
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Ok(ImageInfo {
            id: info.id.unwrap_or_default(),
            tags,
            size,
            created,
        })
    }

    // async fn pull_image(image_name: &str) -> Result<(), AppError> {
    //     let options = Some(CreateImageOptions {
    //         from_image: Some(image_name.to_string()),
    //         ..Default::default()
    //     });

    //     let mut stream = DOCKER.create_image(options, None, None);

    //     while let Some(msg) = stream.next().await {
    //         match msg {
    //             Ok(_) => {}
    //             Err(err) => {
    //                 return Err(AppError::Internal(format!(
    //                     "Failed to create container: {}",
    //                     e
    //                 )))
    //             }
    //         }
    //     }
    //     Ok(())
    // }

    async fn create_temp_container(
        &self,
        prefix: &str,
        image_name: &str,
    ) -> Result<(String, String), AppError> {
        self.pull_image(image_name).await?;
        let name = format!("temp-{}-{}", prefix, uuid::Uuid::new_v4());
        let config = ContainerCreateBody {
            image: Some(image_name.to_string()),
            entrypoint: Some(vec!["/bin/true".to_string()]),
            ..Default::default()
        };
        let options = CreateContainerOptions {
            name: Some(name.clone()),
            platform: "linux/amd64".to_string(),
        };
        let response = DOCKER
            .create_container(Some(options), config)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create container: {}", e)))?;
        Ok((name, response.id))
    }

    async fn remove_container_quiet(container_id: &str) {
        let _ = DOCKER
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
    }

    pub async fn get_file_from_image(
        &self,
        image_name: &str,
        file_path: &str,
    ) -> Result<String, AppError> {
        use futures_util::StreamExt;

        let (_temp_container, container_id) =
            self.create_temp_container("extract", image_name).await?;

        let result = async {
            let download_opts = DownloadFromContainerOptions {
                path: file_path.to_string(),
            };

            let mut stream = DOCKER.download_from_container(&container_id, Some(download_opts));
            let mut all_bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let bytes = chunk
                    .map_err(|e| AppError::Internal(format!("Failed to read archive: {}", e)))?;
                all_bytes.extend_from_slice(&bytes);
            }

            if all_bytes.is_empty() {
                return Err(AppError::NotFound(format!(
                    "File {} not found in image {}",
                    file_path, image_name
                )));
            }

            let target_name = file_path
                .trim_start_matches('/')
                .split('/')
                .last()
                .unwrap_or(file_path);
            let mut archive = tar::Archive::new(std::io::Cursor::new(all_bytes));
            let mut contents = None;
            for entry in archive
                .entries()
                .map_err(|e| AppError::Internal(format!("Failed to read tar archive: {}", e)))?
            {
                let mut entry = entry
                    .map_err(|e| AppError::Internal(format!("Failed to read tar entry: {}", e)))?;
                let path = entry.path().map_err(|e| {
                    AppError::Internal(format!("Failed to read tar entry path: {}", e))
                })?;
                if path.to_string_lossy().trim_start_matches('/') == target_name
                    || path.file_name().and_then(|n| n.to_str()) == Some(target_name)
                {
                    let mut buf = String::new();
                    entry.read_to_string(&mut buf).map_err(|e| {
                        AppError::Internal(format!("Failed to read file contents: {}", e))
                    })?;
                    contents = Some(buf);
                    break;
                }
            }

            contents.ok_or_else(|| {
                AppError::NotFound(format!(
                    "File {} not found in image {}",
                    file_path, image_name
                ))
            })
        }
        .await;

        Self::remove_container_quiet(&container_id).await;
        result
    }

    pub async fn list_directory_in_image(
        &self,
        image_name: &str,
        dir_path: &str,
    ) -> Result<Vec<String>, AppError> {
        use futures_util::StreamExt;

        let (_temp_container, container_id) = self.create_temp_container("ls", image_name).await?;

        let result = async {
            let download_opts = DownloadFromContainerOptions {
                path: dir_path.to_string(),
            };

            let mut stream = DOCKER.download_from_container(&container_id, Some(download_opts));
            let mut all_bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let bytes = chunk
                    .map_err(|e| AppError::Internal(format!("Failed to read archive: {}", e)))?;
                all_bytes.extend_from_slice(&bytes);
            }

            let mut entries = Vec::new();
            if all_bytes.is_empty() {
                return Ok(entries);
            }

            let mut archive = tar::Archive::new(std::io::Cursor::new(all_bytes));
            for entry in archive
                .entries()
                .map_err(|e| AppError::Internal(format!("Failed to read tar archive: {}", e)))?
            {
                let entry = entry
                    .map_err(|e| AppError::Internal(format!("Failed to read tar entry: {}", e)))?;
                let path = entry
                    .path()
                    .map_err(|e| AppError::Internal(format!("Failed to read tar path: {}", e)))?;
                let name = path.to_string_lossy().to_string();
                if !name.is_empty() && name != "." && !name.ends_with('/') {
                    entries.push(name);
                }
            }

            Ok(entries) as Result<Vec<String>, AppError>
        }
        .await;

        Self::remove_container_quiet(&container_id).await;
        result
    }

    pub async fn get_file_from_container(
        &self,
        container_id: &str,
        path: &str,
    ) -> Result<Vec<u8>, AppError> {
        use bollard::container::LogOutput;
        use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};

        let cmd = vec!["cat".to_string(), path.to_string()];

        let exec_options = CreateExecOptions {
            cmd: Some(cmd),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let exec = DOCKER.create_exec(container_id, exec_options).await?;

        let start_options = StartExecOptions {
            detach: false,
            ..Default::default()
        };

        let output = DOCKER.start_exec(&exec.id, Some(start_options)).await?;

        let mut bytes = Vec::new();
        use futures_util::StreamExt;
        match output {
            StartExecResults::Attached { mut output, .. } => {
                while let Some(result) = output.next().await {
                    match result {
                        Ok(log_output) => match log_output {
                            LogOutput::StdOut { message } => bytes.extend_from_slice(&message),
                            LogOutput::StdErr { message } => bytes.extend_from_slice(&message),
                            _ => {}
                        },
                        Err(e) => {
                            return Err(AppError::DockerError {
                                details: e.to_string(),
                            })
                        }
                    }
                }
            }
            StartExecResults::Detached => {
                return Err(AppError::Internal("Exec detached unexpectedly".to_string()))
            }
        }

        let inspect_result = DOCKER.inspect_exec(&exec.id).await?;
        if let Some(code) = inspect_result.exit_code {
            if code != 0 {
                return Err(AppError::Internal(format!(
                    "Cat command exited with code {}",
                    code
                )));
            }
        }

        Ok(bytes)
    }

    pub async fn file_exists_in_container(
        &self,
        container_id: &str,
        path: &str,
    ) -> Result<bool, AppError> {
        match self.get_file_from_container(container_id, path).await {
            Ok(_) => Ok(true),
            Err(AppError::DockerError { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub async fn list_directory_in_container(
        &self,
        container_id: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError> {
        use bollard::container::LogOutput;
        use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};

        let cmd = vec!["ls".to_string(), "-1p".to_string(), path.to_string()];

        let exec_options = CreateExecOptions {
            cmd: Some(cmd),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let exec = DOCKER.create_exec(container_id, exec_options).await?;

        let start_options = StartExecOptions {
            detach: false,
            ..Default::default()
        };

        let output = DOCKER.start_exec(&exec.id, Some(start_options)).await?;

        let mut stdout_bytes = Vec::new();
        use futures_util::StreamExt;
        match output {
            StartExecResults::Attached { mut output, .. } => {
                while let Some(result) = output.next().await {
                    match result {
                        Ok(log_output) => match log_output {
                            LogOutput::StdOut { message } => {
                                stdout_bytes.extend_from_slice(&message)
                            }
                            LogOutput::StdErr { message } => {
                                tracing::warn!("ls stderr: {:?}", String::from_utf8_lossy(&message))
                            }
                            _ => {}
                        },
                        Err(e) => {
                            return Err(AppError::DockerError {
                                details: e.to_string(),
                            })
                        }
                    }
                }
            }
            StartExecResults::Detached => {
                return Err(AppError::Internal("Exec detached unexpectedly".to_string()))
            }
        }

        let inspect_result = DOCKER.inspect_exec(&exec.id).await?;
        if let Some(code) = inspect_result.exit_code {
            if code != 0 {
                return Err(AppError::NotFound(format!(
                    "Directory {} not found in container",
                    path
                )));
            }
        }

        let output_str = String::from_utf8_lossy(&stdout_bytes);
        let entries: Vec<String> = output_str
            .lines()
            .filter(|line| !line.is_empty())
            .filter(|line| !line.ends_with('/'))
            .map(|s| s.to_string())
            .collect();

        Ok(entries)
    }

    pub async fn list_directory_recursive_in_container(
        &self,
        container_id: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError> {
        use bollard::container::LogOutput;
        use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};

        let cmd = vec![
            "find".to_string(),
            path.to_string(),
            "-type".to_string(),
            "f".to_string(),
        ];

        let exec_options = CreateExecOptions {
            cmd: Some(cmd),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let exec = DOCKER.create_exec(container_id, exec_options).await?;

        let start_options = StartExecOptions {
            detach: false,
            ..Default::default()
        };

        let output = DOCKER.start_exec(&exec.id, Some(start_options)).await?;

        let mut stdout_bytes = Vec::new();
        use futures_util::StreamExt;
        match output {
            StartExecResults::Attached { mut output, .. } => {
                while let Some(result) = output.next().await {
                    match result {
                        Ok(log_output) => match log_output {
                            LogOutput::StdOut { message } => {
                                stdout_bytes.extend_from_slice(&message)
                            }
                            LogOutput::StdErr { message } => tracing::warn!(
                                "find stderr: {:?}",
                                String::from_utf8_lossy(&message)
                            ),
                            _ => {}
                        },
                        Err(e) => {
                            return Err(AppError::DockerError {
                                details: e.to_string(),
                            })
                        }
                    }
                }
            }
            StartExecResults::Detached => {
                return Err(AppError::Internal("Exec detached unexpectedly".to_string()))
            }
        }

        let inspect_result = DOCKER.inspect_exec(&exec.id).await?;
        if let Some(code) = inspect_result.exit_code {
            if code != 0 {
                return Err(AppError::NotFound(format!(
                    "Directory {} not found in container",
                    path
                )));
            }
        }

        let output_str = String::from_utf8_lossy(&stdout_bytes);
        let entries: Vec<String> = output_str
            .lines()
            .filter(|line| !line.is_empty())
            .map(|s| s.to_string())
            .collect();

        Ok(entries)
    }

    pub async fn copy_directory_from_container(
        &self,
        container_id: &str,
        container_path: &str,
        host_dest: &str,
    ) -> Result<(), AppError> {
        use futures_util::StreamExt;

        let download_opts = DownloadFromContainerOptions {
            path: container_path.to_string(),
        };

        let mut stream = DOCKER.download_from_container(container_id, Some(download_opts));
        let mut all_bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let bytes =
                chunk.map_err(|e| AppError::Internal(format!("Failed to read archive: {}", e)))?;
            all_bytes.extend_from_slice(&bytes);
        }

        if all_bytes.is_empty() {
            return Err(AppError::Internal(format!(
                "Empty archive for container {} path {}",
                container_id, container_path
            )));
        }

        std::fs::create_dir_all(host_dest)
            .map_err(|e| AppError::Internal(format!("Failed to create host dest dir: {}", e)))?;

        let mut archive = tar::Archive::new(std::io::Cursor::new(all_bytes));
        archive
            .unpack(host_dest)
            .map_err(|e| AppError::Internal(format!("Failed to unpack archive: {}", e)))?;

        Ok(())
    }

    pub async fn copy_directory_from_image(
        &self,
        image_name: &str,
        container_path: &str,
        host_dest: &str,
    ) -> Result<(), AppError> {
        use futures_util::StreamExt;

        self.pull_image(image_name).await?;
        let temp_container = format!("temp-extract-{}", uuid::Uuid::new_v4());

        let config = ContainerCreateBody {
            image: Some(image_name.to_string()),
            entrypoint: Some(vec!["/bin/true".to_string()]),
            ..Default::default()
        };
        let options = CreateContainerOptions {
            name: Some(temp_container.clone()),
            platform: "linux/amd64".to_string(),
        };
        let response = DOCKER
            .create_container(Some(options), config)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create container: {}", e)))?;

        let download_opts = DownloadFromContainerOptions {
            path: container_path.to_string(),
        };

        let mut stream = DOCKER.download_from_container(&response.id, Some(download_opts));
        let mut all_bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let bytes =
                chunk.map_err(|e| AppError::Internal(format!("Failed to read archive: {}", e)))?;
            all_bytes.extend_from_slice(&bytes);
        }

        let _ = DOCKER
            .remove_container(
                &response.id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        if all_bytes.is_empty() {
            tracing::info!(
                "Directory {} not found in image {} (no content to extract)",
                container_path,
                image_name
            );
            return Ok(());
        }

        std::fs::create_dir_all(host_dest)
            .map_err(|e| AppError::Internal(format!("Failed to create host dest dir: {}", e)))?;

        let mut archive = tar::Archive::new(std::io::Cursor::new(all_bytes));
        archive
            .unpack(host_dest)
            .map_err(|e| AppError::Internal(format!("Failed to unpack archive: {}", e)))?;

        tracing::info!(
            "Extracted directory {} from image {} to {}",
            container_path,
            image_name,
            host_dest
        );

        Ok(())
    }
}
