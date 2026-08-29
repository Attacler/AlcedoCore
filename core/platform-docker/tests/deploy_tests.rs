//! Integration tests for DockerPlatform deploy lifecycle.
//!
//! These tests require a running Docker daemon with access to Docker Hub.
//! They skip gracefully when Docker is unavailable.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Once;
use std::time::Duration;

use alcedo_common::config::AppConfig;
use alcedo_container::container::PluginPlatform;
use platform_docker::platform::DockerPlatform;
use platform_docker::runtime::DockerRuntime;

static DOCKER_INIT: Once = Once::new();

fn ensure_docker() {
    DOCKER_INIT.call_once(|| {
        let socket = std::env::var("DOCKER_SOCKET_PATH")
            .unwrap_or_else(|_| "/var/run/docker.sock".to_string());
        platform_docker::init_docker(&socket)
            .expect("Docker daemon must be accessible for deploy tests");
    });
}

fn test_config(dev_mode: bool) -> AppConfig {
    AppConfig {
        database_url: None,
        core_port: 8081,
        local_registry_url: "localhost:5000".to_string(),
        docker_socket: std::env::var("DOCKER_SOCKET_PATH")
            .unwrap_or_else(|_| "/var/run/docker.sock".to_string()),
        plugin_network: String::new(),
        plugins_dir: "/tmp/test-plugins".to_string(),
        health_check_interval: Duration::from_secs(5),
        health_check_timeout: Duration::from_secs(60),
        drain_timeout: Duration::from_secs(60),
        max_restart_attempts: 3,
        shutdown_timeout: Duration::from_secs(30),
        dev_mode,
        redis_url: String::new(),
        capture_body: false,
        capture_body_max_size: 10240,
        nested_field_depth_limit: 5,
        admin_email: None,
        admin_password: None,
        session_ttl_seconds: 86400,
        core_public_url: None,
        system_plugins_url: None,
        rate_limit_auth_requests: 10,
        rate_limit_auth_window: 60,
        rate_limit_api_requests: 100,
        rate_limit_api_window: 60,
        event_forwarder_max_concurrent: 50,
    }
}

fn create_platform(dev_mode: bool) -> DockerPlatform {
    ensure_docker();
    let runtime = Arc::new(DockerRuntime::new()) as Arc<dyn alcedo_container::container::ContainerRuntime>;
    let config = Arc::new(test_config(dev_mode));
    DockerPlatform::new(None, runtime, config)
}

fn unique_slug() -> String {
    format!("test-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap())
}

fn docker_available() -> bool {
    bollard::Docker::connect_with_local_defaults().is_ok()
}

#[tokio::test]
async fn test_deploy_simple_container() {
    if !docker_available() {
        return;
    }

    let platform = create_platform(true);
    let slug = unique_slug();

    let id = platform.deploy(&slug, "1.0.0", "hello-world:latest", HashMap::new()).await
        .expect("Deploy should succeed");

    assert!(!id.is_empty(), "Deployment ID should not be empty");

    platform.remove(&id).await.expect("Remove should succeed");
}

#[tokio::test]
async fn test_deploy_with_env_vars() {
    if !docker_available() {
        return;
    }

    let platform = create_platform(true);
    let slug = unique_slug();
    let mut env = HashMap::new();
    env.insert("FOO".to_string(), "bar".to_string());
    env.insert("HELLO".to_string(), "world".to_string());

    let id = platform.deploy(&slug, "1.0.0", "hello-world:latest", env).await
        .expect("Deploy with env vars should succeed");

    let info = platform_docker::DOCKER.inspect_container(&id, None).await
        .expect("Failed to inspect container");
    let env_vars = info.config.as_ref()
        .and_then(|c| c.env.as_ref())
        .expect("Container should have env vars");
    assert!(env_vars.iter().any(|e| e == "FOO=bar"), "FOO env var should be set");
    assert!(env_vars.iter().any(|e| e == "HELLO=world"), "HELLO env var should be set");

    platform.remove(&id).await.expect("Remove should succeed");
}

#[tokio::test]
async fn test_deploy_remove_nonexistent() {
    if !docker_available() {
        return;
    }

    let platform = create_platform(true);

    let id = "nonexistent-container-id".to_string();
    let err = platform.remove(&id).await.unwrap_err();
    assert!(matches!(err, alcedo_common::AppError::DockerError { .. }), "Should be a Docker error");
}

#[tokio::test]
async fn test_deploy_duplicate() {
    if !docker_available() {
        return;
    }

    let platform = create_platform(true);
    let slug = unique_slug();
    let version = "1.0.0";

    let id1 = platform.deploy(&slug, version, "hello-world:latest", HashMap::new()).await
        .expect("First deploy should succeed");

    let id2 = platform.deploy(&slug, version, "hello-world:latest", HashMap::new()).await
        .expect("Second deploy (same slug+version) should succeed");

    assert_ne!(id1, id2, "Second deploy should create a new container ID");

    platform.remove(&id2).await.expect("Remove should succeed");
}

#[tokio::test]
async fn test_ensure_image() {
    if !docker_available() {
        return;
    }

    let platform = create_platform(true);

    platform.ensure_image("hello-world:latest").await
        .expect("ensure_image should succeed");
}

#[tokio::test]
async fn test_get_address() {
    if !docker_available() {
        return;
    }

    let platform = create_platform(true);
    let slug = unique_slug();

    let id = platform.deploy(&slug, "1.0.0", "hello-world:latest", HashMap::new()).await
        .expect("Deploy should succeed");

    let address = platform.get_address(&id).await
        .expect("get_address should succeed");
    assert!(address.is_some(), "Address should be Some in dev mode");
    assert_eq!(address.unwrap(), "localhost", "Address should be localhost in dev mode");

    platform.remove(&id).await.expect("Remove should succeed");
}

#[tokio::test]
async fn test_restart() {
    if !docker_available() {
        return;
    }

    let platform = create_platform(true);
    let slug = unique_slug();

    let id = platform.deploy(&slug, "1.0.0", "hello-world:latest", HashMap::new()).await
        .expect("Deploy should succeed");

    platform.restart(&id).await.expect("Restart should succeed");

    platform.remove(&id).await.expect("Remove should succeed");
}
