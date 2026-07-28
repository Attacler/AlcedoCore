use async_trait::async_trait;
use http::Request;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, Pod, PodSpec, Service, ServicePort,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{
    AttachParams, DeleteParams, ListParams, LogParams, Patch, PatchParams, PostParams, WatchParams,
};
use kube::core::WatchEvent;
use kube::{Api, Client};
use std::collections::{BTreeMap, HashMap};
use tokio::sync::mpsc;

use pcl::container::{
    ContainerDetails, ContainerInfo, ContainerStatsSnapshot, DeploymentEvent, DeploymentId,
    ImageInfo, InstanceInfo, PluginPlatform,
};
use pcl::AppError;

const MANAGED_BY_LABEL: &str = "app.kubernetes.io/managed-by";
const MANAGED_BY_VALUE: &str = "alcedo-core";
const PART_OF_LABEL: &str = "app.kubernetes.io/part-of";

fn plugin_labels(slug: &str) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert(MANAGED_BY_LABEL.to_string(), MANAGED_BY_VALUE.to_string());
    labels.insert(
        PART_OF_LABEL.to_string(),
        format!("plugin-{}", slug).replace('_', "-"),
    );
    labels
}

pub struct K8sPlatform {
    client: Client,
    namespace: String,
}

impl K8sPlatform {
    pub async fn new() -> Result<Self, AppError> {
        let client = Client::try_default()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create K8s client: {}", e)))?;
        let namespace = std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "default".to_string());
        Ok(Self { client, namespace })
    }

    /// Create a temporary pod that runs a command from a given image.
    /// Returns the pod name so callers can collect logs and clean up.
    async fn create_temp_pod(
        &self,
        prefix: &str,
        image: &str,
        command: Vec<String>,
    ) -> Result<String, AppError> {
        let pod_name = format!("tmp-{}-{}", prefix, uuid::Uuid::new_v4());
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some(pod_name.clone()),
                namespace: Some(self.namespace.clone()),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "extract".to_string(),
                    image: Some(image.to_string()),
                    command: Some(command),
                    image_pull_policy: Some("IfNotPresent".to_string()),
                    ..Default::default()
                }],
                restart_policy: Some("Never".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        pods.create(&PostParams::default(), &pod)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create temp pod: {}", e)))?;
        Ok(pod_name)
    }

    /// Wait for a pod to reach a terminal phase (Succeeded or Failed).
    /// Also detects container-level failures like ImagePullBackOff, CrashLoopBackOff.
    async fn wait_for_pod_completion(&self, name: &str) -> Result<(), AppError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let wp = WatchParams::default()
            .fields(&format!("metadata.name={}", name))
            .timeout(120);
        let mut stream = pods
            .watch(&wp, "0")
            .await
            .map_err(|e| AppError::Internal(format!("Failed to watch pod: {}", e)))?
            .boxed();

        use futures_util::StreamExt;
        use tokio::pin;
        use tokio::time::{sleep, Duration};

        let timeout = sleep(Duration::from_secs(30));
        pin!(timeout);

        loop {
            tokio::select! {
                _ = &mut timeout => {
                    return Err(AppError::Internal("Timeout waiting for temp pod to complete".to_string()));
                }
                result = stream.next() => {
                    match result {
                        Some(Ok(WatchEvent::Added(pod)) | Ok(WatchEvent::Modified(pod))) => {
                            if let Some(status) = &pod.status {
                                // Check container-level failures (ImagePullBackOff, CrashLoopBackOff, etc.)
                                if let Some(containers) = &status.container_statuses {
                                    for cs in containers {
                                        if let Some(state) = &cs.state {
                                            if let Some(waiting) = &state.waiting {
                                                let reason = waiting.reason.as_deref().unwrap_or("");
                                                if reason == "ImagePullBackOff"
                                                    || reason == "ErrImagePull"
                                                    || reason == "CrashLoopBackOff"
                                                    || reason == "CreateContainerConfigError"
                                                {
                                                    return Err(AppError::Internal(format!(
                                                        "Temp pod failed: {} - {}",
                                                        reason,
                                                        waiting.message.as_deref().unwrap_or("unknown"),
                                                    )));
                                                }
                                            }
                                        }
                                    }
                                }
                                match status.phase.as_deref() {
                                    Some("Succeeded") => return Ok(()),
                                    Some("Failed") => {
                                        return Err(AppError::Internal(
                                            "Temp pod failed".to_string(),
                                        ));
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(WatchEvent::Error(e))) => {
                            tracing::warn!("Pod watch error event: {:?}", e);
                        }
                        Some(Err(e)) => {
                            tracing::warn!("Pod watch stream error: {}", e);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Delete a temporary pod, ignoring "not found" errors.
    async fn cleanup_temp_pod(&self, name: &str) {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let _ = pods.delete(name, &DeleteParams::default()).await;
    }

    async fn resolve_pod_for_id(&self, id: &str) -> Result<String, AppError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("{}={}", PART_OF_LABEL, id));
        let list = pods
            .list(&lp)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to list pods: {}", e)))?;
        list.items
            .into_iter()
            .find(|p| p.status.as_ref().and_then(|s| s.phase.as_deref()) == Some("Running"))
            .and_then(|p| p.metadata.name)
            .ok_or_else(|| AppError::NotFound(format!("No running pod for deployment '{}'", id)))
    }
}

#[async_trait]
impl PluginPlatform for K8sPlatform {
    async fn ensure_image(&self, _image: &str) -> Result<(), AppError> {
        Ok(())
    }

    async fn deploy(
        &self,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
    ) -> Result<DeploymentId, AppError> {
        let labels = plugin_labels(slug);
        let match_labels = labels.clone();
        // K8s resource names must follow RFC 1123: lowercase alphanumeric, '-' or '.'
        let deployment_name = format!("plugin-{}", slug).replace('_', "-");

        let env_vars: Vec<EnvVar> = env
            .into_iter()
            .map(|(k, v)| EnvVar {
                name: k,
                value: Some(v),
                ..Default::default()
            })
            .collect();

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some(deployment_name.clone()),
                namespace: Some(self.namespace.clone()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::apps::v1::DeploymentSpec {
                replicas: Some(1),
                selector: k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector {
                    match_labels: Some(match_labels.clone()),
                    ..Default::default()
                },
                template: k8s_openapi::api::core::v1::PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(match_labels),
                        ..Default::default()
                    }),
                    spec: Some(k8s_openapi::api::core::v1::PodSpec {
                        containers: vec![Container {
                            name: slug.to_string(),
                            image: Some(image.to_string()),
                            image_pull_policy: Some("Always".to_string()),
                            ports: vec![ContainerPort {
                                container_port: 8080,
                                ..Default::default()
                            }]
                            .into(),
                            env: env_vars.into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        let deployments: Api<Deployment> = Api::namespaced(self.client.clone(), &self.namespace);
        deployments
            .create(&PostParams::default(), &deployment)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create deployment: {}", e)))?;

        let service = Service {
            metadata: ObjectMeta {
                name: Some(deployment_name.clone()),
                namespace: Some(self.namespace.clone()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                selector: Some(plugin_labels(slug)),
                ports: vec![ServicePort {
                    port: 80,
                    target_port: Some(IntOrString::Int(8080)),
                    ..Default::default()
                }]
                .into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let services: Api<Service> = Api::namespaced(self.client.clone(), &self.namespace);
        services
            .create(&PostParams::default(), &service)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create service: {}", e)))?;

        Ok(deployment_name)
    }

    async fn remove(&self, id: &DeploymentId) -> Result<(), AppError> {
        let deployments: Api<Deployment> = Api::namespaced(self.client.clone(), &self.namespace);
        let services: Api<Service> = Api::namespaced(self.client.clone(), &self.namespace);
        let _ = services.delete(id, &DeleteParams::default()).await;
        deployments
            .delete(id, &DeleteParams::default())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to delete deployment: {}", e)))?;
        Ok(())
    }

    async fn restart(&self, id: &DeploymentId) -> Result<(), AppError> {
        let deployments: Api<Deployment> = Api::namespaced(self.client.clone(), &self.namespace);
        let now = chrono::Utc::now().to_rfc3339();
        let patch = serde_json::json!({
            "spec": {
                "template": {
                    "metadata": {
                        "annotations": {
                            "kubectl.kubernetes.io/restartedAt": now
                        }
                    }
                }
            }
        });
        deployments
            .patch(id, &PatchParams::default(), &Patch::Strategic(patch))
            .await
            .map_err(|e| AppError::Internal(format!("Failed to restart deployment: {}", e)))?;
        Ok(())
    }

    async fn get_address(&self, id: &DeploymentId) -> Result<Option<String>, AppError> {
        let services: Api<Service> = Api::namespaced(self.client.clone(), &self.namespace);
        if let Ok(Some(svc)) = services.get_opt(id).await {
            if let Some(spec) = svc.spec {
                if let Some(cluster_ip) = spec.cluster_ip {
                    if !cluster_ip.is_empty() && cluster_ip != "None" {
                        // Return IP:port — proxy handler uses this directly
                        let port = spec
                            .ports
                            .as_ref()
                            .and_then(|p| p.first())
                            .map(|p| p.port)
                            .unwrap_or(80);
                        return Ok(Some(format!("{}:{}", cluster_ip, port)));
                    }
                }
            }
        }
        let pod_name = self.resolve_pod_for_id(id).await?;
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        if let Ok(Some(pod)) = pods.get_opt(&pod_name).await {
            if let Some(status) = pod.status {
                if let Some(ip) = status.pod_ip {
                    if !ip.is_empty() {
                        return Ok(Some(format!("{}:{}", ip, 8080)));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn is_replicated_service(&self, _id: &DeploymentId) -> bool {
        // Return false so the proxy uses get_address() (ClusterIP) instead
        // of constructing `plugin_{slug}` which uses Swarm DNS naming.
        false
    }

    async fn scale(&self, id: &DeploymentId, replicas: u32) -> Result<(), AppError> {
        let deployments: Api<Deployment> = Api::namespaced(self.client.clone(), &self.namespace);
        let patch = serde_json::json!({ "spec": { "replicas": replicas } });
        deployments
            .patch(id, &PatchParams::default(), &Patch::Strategic(patch))
            .await
            .map_err(|e| AppError::Internal(format!("Failed to scale deployment: {}", e)))?;
        Ok(())
    }

    async fn read_file_from_image(&self, image: &str, path: &str) -> Result<String, AppError> {
        let pod_name = self
            .create_temp_pod("readimg", image, vec!["cat".to_string(), path.to_string()])
            .await?;

        let result = self.wait_for_pod_completion(&pod_name).await;
        let logs = if result.is_ok() {
            let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
            pods.logs(&pod_name, &LogParams::default())
                .await
                .map_err(|e| AppError::Internal(format!("Failed to read pod logs: {}", e)))?
        } else {
            String::new()
        };

        self.cleanup_temp_pod(&pod_name).await;
        result?;
        Ok(logs)
    }

    async fn extract_from_image(&self, image: &str, src: &str, dest: &str) -> Result<(), AppError> {
        // Use tar -C to produce the same output as Docker's download_from_container,
        // which wraps tar entries in the leaf directory name.
        // For /app/public, this produces: public/index.html
        // Extract to dest and the files land at {dest}/public/index.html.
        let dir = std::path::Path::new(src);
        let parent = dir
            .parent()
            .map(|p| p.to_string_lossy())
            .unwrap_or(std::borrow::Cow::Borrowed("/"));
        let base = dir
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or(std::borrow::Cow::Borrowed(""));
        let pod_name = self
            .create_temp_pod(
                "extract",
                image,
                vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!("tar -cf - -C {} {} | base64 -w 0", parent, base),
                ],
            )
            .await?;

        self.wait_for_pod_completion(&pod_name).await?;

        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let b64_output = pods
            .logs(&pod_name, &LogParams::default())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read pod logs: {}", e)))?;

        self.cleanup_temp_pod(&pod_name).await;

        // Decode base64 tar archive and extract to destination
        use base64::Engine;
        let tar_bytes = base64::engine::general_purpose::STANDARD
            .decode(b64_output.trim())
            .map_err(|e| AppError::Internal(format!("Failed to decode tar output: {}", e)))?;

        std::fs::create_dir_all(dest)
            .map_err(|e| AppError::Internal(format!("Failed to create dest dir: {}", e)))?;

        let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
        archive
            .unpack(dest)
            .map_err(|e| AppError::Internal(format!("Failed to unpack tar archive: {}", e)))?;

        Ok(())
    }

    async fn read_file(&self, id: &DeploymentId, path: &str) -> Result<Vec<u8>, AppError> {
        let pod_name = self.resolve_pod_for_id(id).await?;
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let ap = AttachParams {
            stdout: true,
            stderr: true,
            ..Default::default()
        };
        let mut process = pods
            .exec(&pod_name, vec!["cat", path], &ap)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to exec 'cat {}': {}", path, e)))?;

        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        if let Some(mut reader) = process.stdout() {
            reader
                .read_to_end(&mut buf)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to read stdout: {}", e)))?;
        }

        process
            .join()
            .await
            .map_err(|e| AppError::Internal(format!("Exec process failed: {}", e)))?;
        Ok(buf)
    }

    async fn list_directory(&self, id: &DeploymentId, path: &str) -> Result<Vec<String>, AppError> {
        let pod_name = match self.resolve_pod_for_id(id).await {
            Ok(name) => name,
            Err(e) => {
                tracing::error!("[list_directory] resolve_pod_for_id({}) failed: {}", id, e);
                return Err(AppError::NotFound(format!(
                    "Pod not found for deployment '{}'",
                    id
                )));
            }
        };
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let ap = AttachParams {
            stdout: true,
            stderr: true,
            ..Default::default()
        };
        let mut process = match pods.exec(&pod_name, vec!["ls", path], &ap).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(
                    "[list_directory] exec ls '{}' on {} failed: {}",
                    path,
                    pod_name,
                    e
                );
                return Err(AppError::Internal(format!("Failed to exec ls: {}", e)));
            }
        };

        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        if let Some(mut reader) = process.stdout() {
            reader
                .read_to_end(&mut buf)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to read stdout: {}", e)))?;
        }

        process
            .join()
            .await
            .map_err(|e| AppError::Internal(format!("Exec process failed: {}", e)))?;

        Ok(String::from_utf8_lossy(&buf)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }

    async fn health_check(&self) -> Result<(), AppError> {
        let _ = Client::try_default()
            .await
            .map_err(|e| AppError::Internal(format!("K8s health check failed: {}", e)))?;
        Ok(())
    }

    async fn watch_events(&self, _tx: mpsc::Sender<DeploymentEvent>) -> Result<(), AppError> {
        Ok(())
    }

    async fn list_deployments(&self) -> Result<Vec<ContainerInfo>, AppError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let lp = ListParams::default().labels(MANAGED_BY_LABEL);
        let list = pods
            .list(&lp)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to list pods: {}", e)))?;
        Ok(list
            .items
            .into_iter()
            .filter_map(|p| {
                let name = p.metadata.name.clone().unwrap_or_default();
                let status = p
                    .status
                    .as_ref()
                    .and_then(|s| s.phase.as_deref())
                    .unwrap_or("Unknown")
                    .to_string();
                if name.is_empty() {
                    None
                } else {
                    Some(ContainerInfo {
                        id: name.clone(),
                        name,
                        status,
                    })
                }
            })
            .collect())
    }

    async fn inspect(&self, id: &DeploymentId) -> Result<ContainerDetails, AppError> {
        let pod_name = self.resolve_pod_for_id(id).await?;
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let pod = pods
            .get(&pod_name)
            .await
            .map_err(|e| AppError::NotFound(format!("Pod not found: {}", e)))?;
        Ok(ContainerDetails {
            id: pod.metadata.name.unwrap_or_default(),
            name: id.clone(),
            state: pod
                .status
                .as_ref()
                .and_then(|s| s.phase.as_deref())
                .unwrap_or("Unknown")
                .to_string(),
            created: pod
                .metadata
                .creation_timestamp
                .map(|t| t.0.to_rfc3339())
                .unwrap_or_default(),
            image: pod
                .spec
                .as_ref()
                .and_then(|s| s.containers.first())
                .and_then(|c| c.image.clone())
                .unwrap_or_default(),
            network_mode: None,
        })
    }

    async fn list_instances(&self, id: &DeploymentId) -> Result<Vec<InstanceInfo>, AppError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("{}={}", PART_OF_LABEL, id));
        let list = pods
            .list(&lp)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to list pods: {}", e)))?;
        Ok(list
            .items
            .into_iter()
            .map(|p| {
                let name = p.metadata.name.clone().unwrap_or_default();
                let status = p
                    .status
                    .as_ref()
                    .and_then(|s| s.phase.as_deref())
                    .unwrap_or("Unknown")
                    .to_string();
                InstanceInfo {
                    id: name.clone(),
                    status,
                    pod_name: name,
                    container_id: p.status.and_then(|s| s.pod_ip),
                }
            })
            .collect())
    }

    async fn get_instance_logs(
        &self,
        _id: &DeploymentId,
        instance_id: &str,
        tail: usize,
    ) -> Result<String, AppError> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let lp = LogParams {
            tail_lines: Some(tail as i64),
            ..Default::default()
        };
        let logs = pods
            .logs(instance_id, &lp)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to get pod logs: {}", e)))?;
        Ok(logs)
    }

    async fn get_instance_stats(
        &self,
        _id: &DeploymentId,
        instance_id: &str,
    ) -> Result<ContainerStatsSnapshot, AppError> {
        let url = format!(
            "/apis/metrics.k8s.io/v1beta1/namespaces/{}/pods/{}",
            self.namespace, instance_id,
        );

        let request = Request::get(&url)
            .body(Vec::<u8>::new())
            .map_err(|e| AppError::Internal(format!("Failed to build metrics request: {}", e)))?;

        let body: serde_json::Value = match self.client.request(request).await {
            Ok(v) => v,
            Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
                // metrics-server not installed — return empty stats
                tracing::info!(
                    "[K8S_METRICS] Metrics API not available (metrics-server not installed)"
                );
                return Ok(ContainerStatsSnapshot {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    cpu_percent: 0.0,
                    memory_usage_bytes: 0,
                });
            }
            Err(e) => {
                return Err(AppError::Internal(format!(
                    "Metrics API request failed: {}",
                    e
                )))
            }
        };

        let usage = &body["containers"][0]["usage"];
        let cpu_str = usage["cpu"].as_str().unwrap_or("0");
        let mem_str = usage["memory"].as_str().unwrap_or("0");

        Ok(ContainerStatsSnapshot {
            timestamp: chrono::Utc::now().to_rfc3339(),
            cpu_percent: parse_cpu_quantity(cpu_str) * 100.0,
            memory_usage_bytes: parse_memory_bytes(mem_str) as i64,
        })
    }

    async fn list_directory_in_image(
        &self,
        image: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError> {
        let pod_name = self
            .create_temp_pod(
                "lsimg",
                image,
                vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!("ls -1 {} 2>/dev/null || echo ''", path),
                ],
            )
            .await?;

        let result = self.wait_for_pod_completion(&pod_name).await;
        let output = if result.is_ok() {
            let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
            pods.logs(&pod_name, &LogParams::default())
                .await
                .map_err(|e| AppError::Internal(format!("Failed to read pod logs: {}", e)))?
        } else {
            String::new()
        };

        self.cleanup_temp_pod(&pod_name).await;
        Ok(output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    async fn inspect_image(&self, _image: &str) -> Result<ImageInfo, AppError> {
        Err(AppError::Internal(
            "Image inspection not available in K8s mode".to_string(),
        ))
    }

    fn core_url(&self) -> String {
        format!("http://alcedo-core.{}:8080", self.namespace)
    }
}

/// Parse Kubernetes CPU quantity string (e.g. "100m", "0.5", "2") to a fractional CPU value.
fn parse_cpu_quantity(s: &str) -> f64 {
    let s = s.trim();
    if let Some(val) = s.strip_suffix('n') {
        val.parse::<f64>().unwrap_or(0.0) / 1_000_000_000.0
    } else if let Some(val) = s.strip_suffix('m') {
        val.parse::<f64>().unwrap_or(0.0) / 1000.0
    } else if let Some(val) = s.strip_suffix('u') {
        val.parse::<f64>().unwrap_or(0.0) / 1_000_000.0
    } else {
        s.parse::<f64>().unwrap_or(0.0)
    }
}

/// Parse Kubernetes memory quantity string (e.g. "50Mi", "128974848", "1Gi") to bytes.
fn parse_memory_bytes(s: &str) -> u64 {
    let s = s.trim();
    if let Some(val) = s.strip_suffix("Ki") {
        val.parse::<u64>().unwrap_or(0) * 1024
    } else if let Some(val) = s.strip_suffix("Mi") {
        val.parse::<u64>().unwrap_or(0) * 1024 * 1024
    } else if let Some(val) = s.strip_suffix("Gi") {
        val.parse::<u64>().unwrap_or(0) * 1024 * 1024 * 1024
    } else if let Some(val) = s.strip_suffix("Ti") {
        val.parse::<u64>().unwrap_or(0) * 1024 * 1024 * 1024 * 1024
    } else if let Some(val) = s.strip_suffix("k") {
        val.parse::<u64>().unwrap_or(0) * 1000
    } else if let Some(val) = s.strip_suffix("M") {
        val.parse::<u64>().unwrap_or(0) * 1000 * 1000
    } else if let Some(val) = s.strip_suffix("G") {
        val.parse::<u64>().unwrap_or(0) * 1000 * 1000 * 1000
    } else if let Some(val) = s.strip_suffix("T") {
        val.parse::<u64>().unwrap_or(0) * 1000 * 1000 * 1000 * 1000
    } else {
        s.parse::<u64>().unwrap_or(0)
    }
}
