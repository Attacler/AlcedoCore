use std::collections::HashMap;
use std::time::Duration;

use kube::Api;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Service;
use alcedo_container::container::{DeploymentId, PluginPlatform};
use platform_k8s::platform::K8sPlatform;

fn k8s_available() -> bool {
    std::thread::spawn(|| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async { kube::Client::try_default().await.is_ok() })
    })
    .join()
    .unwrap_or(false)
}

async fn create_test_platform() -> Option<K8sPlatform> {
    K8sPlatform::new().await.ok()
}

fn test_namespace() -> String {
    std::env::var("POD_NAMESPACE").unwrap_or_else(|_| "default".to_string())
}

async fn wait_for_pod_ready(platform: &K8sPlatform, id: &DeploymentId) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 30 {
        if platform.inspect(id).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

#[tokio::test]
async fn test_k8s_platform_connects() {
    if !k8s_available() {
        return;
    }
    let platform = create_test_platform().await;
    assert!(platform.is_some(), "Platform should be created when K8s is available");
}

#[tokio::test]
async fn test_deploy_creates_deployment_and_service() {
    if !k8s_available() {
        return;
    }
    let platform = create_test_platform().await.unwrap();
    let slug = format!(
        "test-{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let version = "1.0.0";
    let image = "nginx:alpine";

    let deployment_id =
        platform.deploy(&slug, &version, image, HashMap::new()).await
            .expect("Deploy should succeed");
    assert!(!deployment_id.is_empty(), "Deploy should return a deployment ID");

    assert!(
        wait_for_pod_ready(&platform, &deployment_id).await,
        "Pod should become Running within timeout"
    );

    let details = platform.inspect(&deployment_id).await
        .expect("Inspect should succeed");
    assert_eq!(details.name, deployment_id);
    assert!(!details.state.is_empty());

    platform.remove(&deployment_id).await
        .expect("Remove should succeed");
}

#[tokio::test]
async fn test_deploy_with_env_vars() {
    if !k8s_available() {
        return;
    }
    let platform = create_test_platform().await.unwrap();
    let namespace = test_namespace();
    let slug = format!(
        "testenv-{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let version = "1.0.0";
    let image = "nginx:alpine";
    let mut env = HashMap::new();
    env.insert("MY_VAR".to_string(), "my_value".to_string());
    env.insert("ANOTHER_VAR".to_string(), "42".to_string());

    let deployment_id = platform.deploy(&slug, &version, image, env).await
        .expect("Deploy should succeed");

    // Verify env vars in the Deployment spec via kube API
    let client = kube::Client::try_default().await.unwrap();
    let deployments: Api<Deployment> = Api::namespaced(client, &namespace);
    let deployment = deployments.get(&deployment_id).await
        .expect("Deployment should exist in K8s");

    let containers = deployment
        .spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .map(|s| &s.containers)
        .expect("Deployment should have containers");
    assert_eq!(containers.len(), 1);

    let container = &containers[0];
    let env_vars = container.env.as_ref().expect("Container should have env vars");
    let env_map: HashMap<&str, &str> = env_vars
        .iter()
        .filter_map(|e| e.value.as_ref().map(|v| (e.name.as_str(), v.as_str())))
        .collect();
    assert_eq!(env_map.get("MY_VAR"), Some(&"my_value"));
    assert_eq!(env_map.get("ANOTHER_VAR"), Some(&"42"));

    platform.remove(&deployment_id).await
        .expect("Remove should succeed");
}

#[tokio::test]
async fn test_ensure_image() {
    if !k8s_available() {
        return;
    }
    let platform = create_test_platform().await.unwrap();
    let result = platform.ensure_image("nginx:alpine").await;
    assert!(result.is_ok(), "ensure_image should be a no-op that returns Ok");
}

#[tokio::test]
async fn test_remove_cleans_up() {
    if !k8s_available() {
        return;
    }
    let platform = create_test_platform().await.unwrap();
    let namespace = test_namespace();
    let slug = format!(
        "testrm-{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let version = "1.0.0";
    let image = "nginx:alpine";

    let deployment_id =
        platform.deploy(&slug, &version, image, HashMap::new()).await
            .expect("Deploy should succeed");

    let client = kube::Client::try_default().await.unwrap();
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
    let services: Api<Service> = Api::namespaced(client, &namespace);

    // Verify resources exist before removal
    assert!(
        deployments.get(&deployment_id).await.is_ok(),
        "Deployment should exist before remove"
    );
    assert!(
        services.get(&deployment_id).await.is_ok(),
        "Service should exist before remove"
    );

    platform.remove(&deployment_id).await
        .expect("Remove should succeed");

    // Verify Deployment is deleted
    assert!(
        deployments.get(&deployment_id).await.is_err(),
        "Deployment should be deleted after remove"
    );
}

#[tokio::test]
async fn test_restart() {
    if !k8s_available() {
        return;
    }
    let platform = create_test_platform().await.unwrap();
    let slug = format!(
        "testrst-{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let version = "1.0.0";
    let image = "nginx:alpine";

    let deployment_id =
        platform.deploy(&slug, &version, image, HashMap::new()).await
            .expect("Deploy should succeed");
    assert!(
        wait_for_pod_ready(&platform, &deployment_id).await,
        "Pod should become Running"
    );

    platform.restart(&deployment_id).await
        .expect("Restart should succeed");

    let details = platform.inspect(&deployment_id).await
        .expect("Inspect should succeed after restart");
    assert_eq!(details.name, deployment_id);

    platform.remove(&deployment_id).await
        .expect("Remove should succeed");
}

#[tokio::test]
async fn test_get_address() {
    if !k8s_available() {
        return;
    }
    let platform = create_test_platform().await.unwrap();
    let slug = format!(
        "testaddr-{}",
        uuid::Uuid::new_v4().to_string().split('-').next().unwrap()
    );
    let version = "1.0.0";
    let image = "nginx:alpine";

    let deployment_id =
        platform.deploy(&slug, &version, image, HashMap::new()).await
            .expect("Deploy should succeed");
    assert!(
        wait_for_pod_ready(&platform, &deployment_id).await,
        "Pod should become Running"
    );

    let address = platform.get_address(&deployment_id).await
        .expect("get_address should succeed");
    assert!(address.is_some(), "get_address should return an address");
    let addr = address.unwrap();
    assert!(!addr.is_empty(), "Address should not be empty");
    assert!(addr.contains(':'), "Address should be in IP:port format");

    platform.remove(&deployment_id).await
        .expect("Remove should succeed");
}
