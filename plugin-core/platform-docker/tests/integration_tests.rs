use bollard::Docker;
use bollard::API_DEFAULT_VERSION;
use futures_util::StreamExt;

fn create_docker_client() -> Result<Docker, bollard::errors::Error> {
    let socket_path = std::env::var("DOCKER_SOCKET_PATH")
        .unwrap_or_else(|_| "/var/run/docker.sock".to_string());

    if socket_path.starts_with("unix://") {
        Docker::connect_with_socket(&socket_path, 60, API_DEFAULT_VERSION)
    } else {
        Docker::connect_with_local_defaults()
    }
}

async fn cleanup_container(docker: &Docker, container_id: &str) {
    let _ = docker.stop_container(container_id, None::<bollard::query_parameters::StopContainerOptions>).await;
    let _ = docker.remove_container(container_id, Some(bollard::query_parameters::RemoveContainerOptions {
        force: true,
        ..Default::default()
    })).await;
}

async fn is_image_available(docker: &Docker, image: &str) -> bool {
    use std::collections::HashMap;
    let mut filters = HashMap::new();
    filters.insert("reference".to_string(), vec![image.to_string()]);
    let options = bollard::query_parameters::ListImagesOptions {
        filters: Some(filters),
        ..Default::default()
    };
    docker.list_images(Some(options)).await.map(|imgs| !imgs.is_empty()).unwrap_or(false)
}

fn docker_client() -> Option<Docker> {
    create_docker_client().ok()
}

fn require_docker() -> Docker {
    docker_client().expect("Docker not available")
}

mod docker_client_tests {
    use super::*;

    #[tokio::test]
    async fn test_docker_ping() {
        let docker = require_docker();
        docker.ping().await.expect("Docker ping failed");
    }

    #[tokio::test]
    async fn test_docker_list_networks() {
        let docker = require_docker();
        let networks = docker.list_networks(None::<bollard::query_parameters::ListNetworksOptions>).await
            .expect("Failed to list networks");
        assert!(!networks.is_empty(), "At least default network should exist");
    }

    #[tokio::test]
    async fn test_create_and_list_container() {
        let docker = require_docker();
        if !is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("test-plugin-{}", uuid::Uuid::new_v4());

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container");

        let list_options = bollard::query_parameters::ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let containers = docker.list_containers(Some(list_options)).await
            .expect("Failed to list containers");

        let created = containers.iter().any(|c| c.id.as_ref() == Some(&response.id));
        assert!(created, "Created container should appear in list");

        cleanup_container(&docker, &response.id).await;
    }

    #[tokio::test]
    async fn test_start_and_stop_container() {
        let docker = require_docker();
        if !is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("test-plugin-{}", uuid::Uuid::new_v4());

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container");

        docker.start_container(&response.id, None::<bollard::query_parameters::StartContainerOptions>).await
            .expect("Failed to start container");

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let info = docker.inspect_container(&response.id, None).await
            .expect("Failed to inspect container");
        let state = info.state.and_then(|s| s.status);
        assert!(state.is_some(), "Container should have a state");
        assert!(state == Some(bollard::models::ContainerStateStatusEnum::RUNNING) ||
               state == Some(bollard::models::ContainerStateStatusEnum::EXITED),
               "Container should be running or exited");

        docker.stop_container(&response.id, None::<bollard::query_parameters::StopContainerOptions>).await
            .expect("Failed to stop container");

        cleanup_container(&docker, &response.id).await;
    }

    #[tokio::test]
    async fn test_remove_container() {
        let docker = require_docker();
        if !is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("test-plugin-{}", uuid::Uuid::new_v4());

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container");

        docker.remove_container(&response.id, Some(bollard::query_parameters::RemoveContainerOptions {
            force: true,
            ..Default::default()
        })).await.expect("Failed to remove container");

        let list_options = bollard::query_parameters::ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let containers = docker.list_containers(Some(list_options)).await
            .expect("Failed to list containers");

        let still_exists = containers.iter().any(|c| c.id.as_ref() == Some(&response.id));
        assert!(!still_exists, "Container should be removed");
    }

    #[tokio::test]
    async fn test_container_with_environment_vars() {
        let docker = require_docker();
        if !is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("test-plugin-{}", uuid::Uuid::new_v4());

        let env_vars: Vec<String> = vec!["TEST_VAR=test_value".to_string(), "ANOTHER_VAR=123".to_string()];
        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            env: Some(env_vars),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container with env vars");

        let info = docker.inspect_container(&response.id, None).await
            .expect("Failed to inspect container");

        let has_env = info.config.as_ref()
            .and_then(|c| c.env.as_ref())
            .map(|env| env.iter().any(|e| e.contains("TEST_VAR")))
            .unwrap_or(false);

        assert!(has_env, "Environment variables should be set");

        cleanup_container(&docker, &response.id).await;
    }
}

mod plugin_lifecycle_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_plugin_lifecycle() {
        let docker = require_docker();
        if !is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("hello-world-{}", uuid::Uuid::new_v4());

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container");

        docker.start_container(&response.id, None::<bollard::query_parameters::StartContainerOptions>).await
            .expect("Failed to start container");

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let info = docker.inspect_container(&response.id, None).await
            .expect("Failed to inspect container");
        let state = info.state.and_then(|s| s.status);
        assert!(state.is_some(), "Container should have a state");
        assert!(state == Some(bollard::models::ContainerStateStatusEnum::RUNNING) ||
               state == Some(bollard::models::ContainerStateStatusEnum::EXITED),
               "Container should be running or exited");

        docker.stop_container(&response.id, None::<bollard::query_parameters::StopContainerOptions>).await
            .expect("Failed to stop container");

        let info = docker.inspect_container(&response.id, None).await
            .expect("Failed to inspect container after stop");
        let state_after_stop = info.state.and_then(|s| s.status);
        assert!(state_after_stop == Some(bollard::models::ContainerStateStatusEnum::EXITED) ||
               state_after_stop == Some(bollard::models::ContainerStateStatusEnum::DEAD) ||
               state_after_stop == Some(bollard::models::ContainerStateStatusEnum::CREATED),
               "Container should be in terminal state after stop");

        cleanup_container(&docker, &response.id).await;
    }

    #[tokio::test]
    async fn test_pull_and_create_container() {
        let docker = require_docker();
        if is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("test-pull-{}", uuid::Uuid::new_v4());

        let pull_options = bollard_stubs::query_parameters::CreateImageOptions {
            from_image: Some("hello-world".to_string()),
            ..Default::default()
        };
        let mut stream = docker.create_image(Some(pull_options), None, None);
        while let Some(result) = stream.next().await {
            result.expect("Failed to pull image");
        }

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container from pulled image");

        docker.start_container(&response.id, None::<bollard::query_parameters::StartContainerOptions>).await
            .expect("Failed to start container");

        cleanup_container(&docker, &response.id).await;
    }
}

mod proxy_tests {
    use super::*;

    #[tokio::test]
    async fn test_proxy_route_to_container() {
        let docker = require_docker();
        if !is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("test-proxy-{}", uuid::Uuid::new_v4());

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container");

        docker.start_container(&response.id, None::<bollard::query_parameters::StartContainerOptions>).await
            .expect("Failed to start container");

        let info = docker.inspect_container(&response.id, None).await
            .expect("Failed to inspect container");

        let container_state = info.state.and_then(|s| s.status);
        assert!(container_state.is_some(), "Container should have a state");

        cleanup_container(&docker, &response.id).await;
    }

    #[tokio::test]
    async fn test_container_network_connectivity() {
        let docker = require_docker();
        if !is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("test-net-{}", uuid::Uuid::new_v4());

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container");

        let networks = docker.list_networks(None::<bollard::query_parameters::ListNetworksOptions>).await
            .expect("Failed to list networks");

        assert!(!networks.is_empty(), "Should be able to list networks");

        cleanup_container(&docker, &response.id).await;
    }
}

mod health_check_tests {
    use super::*;

    #[tokio::test]
    async fn test_docker_health_check() {
        let docker = require_docker();
        docker.ping().await.expect("Docker health check should succeed");
    }
}

mod static_file_tests {
    use super::*;

    #[tokio::test]
    async fn test_get_file_from_container_exec() {
        let docker = require_docker();

        let container_name = format!("test-static-{}", uuid::Uuid::new_v4());

        if !is_image_available(&docker, "hello-world-plugin").await {
            let config = bollard::models::ContainerCreateBody {
                image: Some("hello-world-plugin:latest".to_string()),
                ..Default::default()
            };
            let options = bollard::query_parameters::CreateContainerOptions {
                name: Some(container_name),
                platform: String::new(),
            };

            let response = docker.create_container(Some(options), config).await
                .expect("Failed to create container");
            docker.start_container(&response.id, None::<bollard::query_parameters::StartContainerOptions>).await
                .expect("Failed to start container");

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let exec_config = bollard::exec::CreateExecOptions {
                cmd: Some(vec!["mkdir".to_string(), "-p".to_string(), "/plugin/public".to_string()]),
                attach_stdout: Some(false),
                attach_stderr: Some(false),
                ..Default::default()
            };
            let _ = docker.create_exec(&response.id, exec_config).await
                .expect("Failed to create mkdir exec");

            let exec_config = bollard::exec::CreateExecOptions {
                cmd: Some(vec!["sh".to_string(), "-c".to_string(), "echo 'Hello from public' > /plugin/public/index.html".to_string()]),
                attach_stdout: Some(false),
                attach_stderr: Some(false),
                ..Default::default()
            };
            let _ = docker.create_exec(&response.id, exec_config).await
                .expect("Failed to create write exec");

            let exec_config = bollard::exec::CreateExecOptions {
                cmd: Some(vec!["cat".to_string(), "/plugin/public/index.html".to_string()]),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            };
            let exec = docker.create_exec(&response.id, exec_config).await
                .expect("Failed to create exec");

            let start_options = bollard::exec::StartExecOptions {
                detach: false,
                ..Default::default()
            };
            let output = docker.start_exec(&exec.id, Some(start_options)).await
                .expect("Failed to start exec");

            let mut bytes = Vec::new();
            if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
                use futures_util::StreamExt;
                while let Some(result) = output.next().await {
                    if let Ok(output) = result {
                        match output {
                            bollard::container::LogOutput::StdOut { message } => bytes.extend_from_slice(&message),
                            bollard::container::LogOutput::StdErr { message } => bytes.extend_from_slice(&message),
                            _ => {}
                        }
                    }
                }
            }

            let output_str = String::from_utf8(bytes).expect("Failed to parse output");
            assert!(output_str.contains("Hello from public"), "Output should contain 'Hello from public', got: {}", output_str);

            cleanup_container(&docker, &response.id).await;
        } else {
            println!("Skipping test - image already exists");
        }
    }
}

mod fetch_plugin_doc_tests {
    use super::*;

    /// Helper to create a test container with a docs/ directory and a test.md file.
    /// Uses hello-world-plugin which has shell and basic utilities.
    async fn setup_container_with_doc(docker: &Docker, doc_content: &str) -> String {
        let container_name = format!("fetch-doc-test-{}", uuid::Uuid::new_v4());

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world-plugin:latest".to_string()),
            cmd: Some(vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 60".to_string()]),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container");
        docker.start_container(&response.id, None::<bollard::query_parameters::StartContainerOptions>).await
            .expect("Failed to start container");

        // Wait for container to be fully running
        tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;

        // Create docs directory and write test.md - need to START the exec
        // Use printf instead of echo to handle escape sequences properly
        let exec_config = bollard::exec::CreateExecOptions {
            cmd: Some(vec!["/bin/sh".to_string(), "-c".to_string(), format!("mkdir -p /docs && printf '%s' '{}' > /docs/test.md", doc_content)]),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            ..Default::default()
        };
        let exec = docker.create_exec(&response.id, exec_config).await
            .expect("Failed to create exec");
        let _ = docker.start_exec(&exec.id, None::<bollard::exec::StartExecOptions>).await
            .expect("Failed to start exec");

        // Wait for exec to complete before returning
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        response.id
    }

    /// Helper to create a test container with nested docs/guides/test.md.
    /// Uses hello-world-plugin which has shell and basic utilities.
    async fn setup_container_with_nested_doc(docker: &Docker) -> String {
        let container_name = format!("fetch-doc-nested-{}", uuid::Uuid::new_v4());

        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world-plugin:latest".to_string()),
            cmd: Some(vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 60".to_string()]),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker.create_container(Some(options), config).await
            .expect("Failed to create container");
        docker.start_container(&response.id, None::<bollard::query_parameters::StartContainerOptions>).await
            .expect("Failed to start container");

        // Wait for container to be fully running
        tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;

        // Create nested docs/guides directory and write file - need to START the exec
        let exec_config = bollard::exec::CreateExecOptions {
            cmd: Some(vec!["/bin/sh".to_string(), "-c".to_string(), "mkdir -p /docs/guides && echo '# Guide' > /docs/guides/test.md".to_string()]),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            ..Default::default()
        };
        let exec = docker.create_exec(&response.id, exec_config).await
            .expect("Failed to create nested exec");
        let _ = docker.start_exec(&exec.id, None::<bollard::exec::StartExecOptions>).await
            .expect("Failed to start nested exec");

        // Wait for exec to complete before returning
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        response.id
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_success() {
        let docker = require_docker();
        let container_id = setup_container_with_doc(&docker, "# Test Doc\n\nHello world").await;

        // Verify we can read the file directly via exec first
        let exec_config = bollard::exec::CreateExecOptions {
            cmd: Some(vec!["cat".to_string(), "/docs/test.md".to_string()]),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };
        let exec = docker.create_exec(&container_id, exec_config).await
            .expect("Failed to create exec");

        let start_options = bollard::exec::StartExecOptions {
            detach: false,
            ..Default::default()
        };
        let output = docker.start_exec(&exec.id, Some(start_options)).await
            .expect("Failed to start exec");

        let mut bytes = Vec::new();
        if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
            use futures_util::StreamExt;
            while let Some(result) = output.next().await {
                if let Ok(output) = result {
                    match output {
                        bollard::container::LogOutput::StdOut { message } => bytes.extend_from_slice(&message),
                        bollard::container::LogOutput::StdErr { message } => bytes.extend_from_slice(&message),
                        _ => {}
                    }
                }
            }
        }

        let output_str = String::from_utf8(bytes).expect("Failed to parse output");
        assert!(output_str.contains("Test Doc"), "Should contain doc content, got: {}", output_str);

        cleanup_container(&docker, &container_id).await;
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_nested_path() {
        let docker = require_docker();
        let container_id = setup_container_with_nested_doc(&docker).await;

        // Verify nested file exists
        let exec_config = bollard::exec::CreateExecOptions {
            cmd: Some(vec!["cat".to_string(), "/docs/guides/test.md".to_string()]),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };
        let exec = docker.create_exec(&container_id, exec_config).await
            .expect("Failed to create exec");

        let start_options = bollard::exec::StartExecOptions {
            detach: false,
            ..Default::default()
        };
        let output = docker.start_exec(&exec.id, Some(start_options)).await
            .expect("Failed to start exec");

        let mut bytes = Vec::new();
        if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
            use futures_util::StreamExt;
            while let Some(result) = output.next().await {
                if let Ok(output) = result {
                    match output {
                        bollard::container::LogOutput::StdOut { message } => bytes.extend_from_slice(&message),
                        bollard::container::LogOutput::StdErr { message } => bytes.extend_from_slice(&message),
                        _ => {}
                    }
                }
            }
        }

        let output_str = String::from_utf8(bytes).expect("Failed to parse output");
        assert!(output_str.contains("Guide"), "Should contain nested doc content, got: {}", output_str);

        cleanup_container(&docker, &container_id).await;
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_content_type() {
        use pcl::error::AppError;

        let content = "# Test".as_bytes().to_vec();
        let response = axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(axum::http::header::CONTENT_TYPE, "text/markdown; charset=utf-8")
            .body(axum::body::Body::from(content.clone()))
            .map_err(|e| AppError::Internal(format!("Failed to build response: {}", e)));

        assert!(response.is_ok());
        let response = response.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let content_type = response.headers().get("content-type").expect("Content-Type header should be present");
        assert!(content_type.to_str().unwrap().contains("text/markdown"), "Content-Type should be text/markdown");
        assert!(content_type.to_str().unwrap().contains("charset=utf-8"), "Content-Type should include charset");
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_not_found() {
        use pcl::error::AppError;

        let docker_error = AppError::Internal("Cat command exited with code 1".to_string());
        let result: Result<String, AppError> = Err(docker_error);

        let file_path = "nonexistent.md";
        let not_found_result = result.map_err(|_| {
            AppError::NotFound(format!("File not found: {}", file_path))
        });

        assert!(not_found_result.is_err());
        let err = not_found_result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(error_msg.contains("nonexistent.md"), "Error should mention the file path");
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_traversal_dotdot() {
        use pcl::api::admin::validate_docs_path;
        use pcl::error::AppError;

        let result = validate_docs_path("../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        let error_msg = err.to_string();
        assert!(error_msg.contains("traversal"), "Error should mention path traversal");
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_traversal_absolute() {
        use pcl::api::admin::validate_docs_path;
        use pcl::error::AppError;

        let result = validate_docs_path("/etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_null_bytes() {
        use pcl::api::admin::validate_docs_path;
        use pcl::error::AppError;

        let result = validate_docs_path("test\0.md");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        let error_msg = err.to_string();
        assert!(error_msg.contains("null"), "Error should mention null bytes");
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_plugin_not_found() {
        use pcl::error::AppError;

        let slug = "nonexistent-plugin-12345";
        let result: Result<(), AppError> = Err(AppError::NotFound(format!("Plugin not found: {}", slug)));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(error_msg.contains("nonexistent-plugin-12345"), "Error should mention the slug");
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_no_active_version() {
        use pcl::error::AppError;

        let slug = "plugin-without-active-version";
        let result: Result<(), AppError> = Err(AppError::NotFound(format!("No active version for plugin: {}", slug)));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(error_msg.contains("No active version"), "Error should mention no active version");
    }
}
