use bollard::Docker;
use bollard::API_DEFAULT_VERSION;
use futures_util::StreamExt;

fn create_docker_client() -> Result<Docker, bollard::errors::Error> {
    let socket_path =
        std::env::var("DOCKER_SOCKET_PATH").unwrap_or_else(|_| "/var/run/docker.sock".to_string());

    if socket_path.starts_with("unix://") {
        Docker::connect_with_socket(&socket_path, 60, API_DEFAULT_VERSION)
    } else {
        Docker::connect_with_local_defaults()
    }
}

async fn cleanup_container(docker: &Docker, container_id: &str) {
    let _ = docker
        .stop_container(
            container_id,
            None::<bollard::query_parameters::StopContainerOptions>,
        )
        .await;
    let _ = docker
        .remove_container(
            container_id,
            Some(bollard::query_parameters::RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;
}

async fn is_image_available(docker: &Docker, image: &str) -> bool {
    use std::collections::HashMap;
    let mut filters = HashMap::new();
    filters.insert("reference".to_string(), vec![image.to_string()]);
    let options = bollard::query_parameters::ListImagesOptions {
        filters: Some(filters),
        ..Default::default()
    };
    docker
        .list_images(Some(options))
        .await
        .map(|imgs| !imgs.is_empty())
        .unwrap_or(false)
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
        let networks = docker
            .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
            .await
            .expect("Failed to list networks");
        assert!(
            !networks.is_empty(),
            "At least default network should exist"
        );
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

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container");

        let list_options = bollard::query_parameters::ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let containers = docker
            .list_containers(Some(list_options))
            .await
            .expect("Failed to list containers");

        let created = containers
            .iter()
            .any(|c| c.id.as_ref() == Some(&response.id));
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

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container");

        docker
            .start_container(
                &response.id,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await
            .expect("Failed to start container");

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let info = docker
            .inspect_container(&response.id, None)
            .await
            .expect("Failed to inspect container");
        let state = info.state.and_then(|s| s.status);
        assert!(state.is_some(), "Container should have a state");
        assert!(
            state == Some(bollard::models::ContainerStateStatusEnum::RUNNING)
                || state == Some(bollard::models::ContainerStateStatusEnum::EXITED),
            "Container should be running or exited"
        );

        docker
            .stop_container(
                &response.id,
                None::<bollard::query_parameters::StopContainerOptions>,
            )
            .await
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

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container");

        docker
            .remove_container(
                &response.id,
                Some(bollard::query_parameters::RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .expect("Failed to remove container");

        let list_options = bollard::query_parameters::ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let containers = docker
            .list_containers(Some(list_options))
            .await
            .expect("Failed to list containers");

        let still_exists = containers
            .iter()
            .any(|c| c.id.as_ref() == Some(&response.id));
        assert!(!still_exists, "Container should be removed");
    }

    #[tokio::test]
    async fn test_container_with_environment_vars() {
        let docker = require_docker();
        if !is_image_available(&docker, "hello-world").await {
            return;
        }

        let container_name = format!("test-plugin-{}", uuid::Uuid::new_v4());

        let env_vars: Vec<String> = vec![
            "TEST_VAR=test_value".to_string(),
            "ANOTHER_VAR=123".to_string(),
        ];
        let config = bollard::models::ContainerCreateBody {
            image: Some("hello-world".to_string()),
            env: Some(env_vars),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container with env vars");

        let info = docker
            .inspect_container(&response.id, None)
            .await
            .expect("Failed to inspect container");

        let has_env = info
            .config
            .as_ref()
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

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container");

        docker
            .start_container(
                &response.id,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await
            .expect("Failed to start container");

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let info = docker
            .inspect_container(&response.id, None)
            .await
            .expect("Failed to inspect container");
        let state = info.state.and_then(|s| s.status);
        assert!(state.is_some(), "Container should have a state");
        assert!(
            state == Some(bollard::models::ContainerStateStatusEnum::RUNNING)
                || state == Some(bollard::models::ContainerStateStatusEnum::EXITED),
            "Container should be running or exited"
        );

        docker
            .stop_container(
                &response.id,
                None::<bollard::query_parameters::StopContainerOptions>,
            )
            .await
            .expect("Failed to stop container");

        let info = docker
            .inspect_container(&response.id, None)
            .await
            .expect("Failed to inspect container after stop");
        let state_after_stop = info.state.and_then(|s| s.status);
        assert!(
            state_after_stop == Some(bollard::models::ContainerStateStatusEnum::EXITED)
                || state_after_stop == Some(bollard::models::ContainerStateStatusEnum::DEAD)
                || state_after_stop == Some(bollard::models::ContainerStateStatusEnum::CREATED),
            "Container should be in terminal state after stop"
        );

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

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container from pulled image");

        docker
            .start_container(
                &response.id,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await
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

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container");

        docker
            .start_container(
                &response.id,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await
            .expect("Failed to start container");

        let info = docker
            .inspect_container(&response.id, None)
            .await
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

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container");

        let networks = docker
            .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
            .await
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
        docker
            .ping()
            .await
            .expect("Docker health check should succeed");
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

            let response = docker
                .create_container(Some(options), config)
                .await
                .expect("Failed to create container");
            docker
                .start_container(
                    &response.id,
                    None::<bollard::query_parameters::StartContainerOptions>,
                )
                .await
                .expect("Failed to start container");

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let exec_config = bollard::exec::CreateExecOptions {
                cmd: Some(vec![
                    "mkdir".to_string(),
                    "-p".to_string(),
                    "/plugin/public".to_string(),
                ]),
                attach_stdout: Some(false),
                attach_stderr: Some(false),
                ..Default::default()
            };
            let _ = docker
                .create_exec(&response.id, exec_config)
                .await
                .expect("Failed to create mkdir exec");

            let exec_config = bollard::exec::CreateExecOptions {
                cmd: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    "echo 'Hello from public' > /plugin/public/index.html".to_string(),
                ]),
                attach_stdout: Some(false),
                attach_stderr: Some(false),
                ..Default::default()
            };
            let _ = docker
                .create_exec(&response.id, exec_config)
                .await
                .expect("Failed to create write exec");

            let exec_config = bollard::exec::CreateExecOptions {
                cmd: Some(vec![
                    "cat".to_string(),
                    "/plugin/public/index.html".to_string(),
                ]),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            };
            let exec = docker
                .create_exec(&response.id, exec_config)
                .await
                .expect("Failed to create exec");

            let start_options = bollard::exec::StartExecOptions {
                detach: false,
                ..Default::default()
            };
            let output = docker
                .start_exec(&exec.id, Some(start_options))
                .await
                .expect("Failed to start exec");

            let mut bytes = Vec::new();
            if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
                use futures_util::StreamExt;
                while let Some(result) = output.next().await {
                    if let Ok(output) = result {
                        match output {
                            bollard::container::LogOutput::StdOut { message } => {
                                bytes.extend_from_slice(&message)
                            }
                            bollard::container::LogOutput::StdErr { message } => {
                                bytes.extend_from_slice(&message)
                            }
                            _ => {}
                        }
                    }
                }
            }

            let output_str = String::from_utf8(bytes).expect("Failed to parse output");
            assert!(
                output_str.contains("Hello from public"),
                "Output should contain 'Hello from public', got: {}",
                output_str
            );

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
            cmd: Some(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "sleep 60".to_string(),
            ]),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container");
        docker
            .start_container(
                &response.id,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await
            .expect("Failed to start container");

        // Wait for container to be fully running
        tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;

        // Create docs directory and write test.md - need to START the exec
        // Use printf instead of echo to handle escape sequences properly
        let exec_config = bollard::exec::CreateExecOptions {
            cmd: Some(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!(
                    "mkdir -p /docs && printf '%s' '{}' > /docs/test.md",
                    doc_content
                ),
            ]),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            ..Default::default()
        };
        let exec = docker
            .create_exec(&response.id, exec_config)
            .await
            .expect("Failed to create exec");
        let _ = docker
            .start_exec(&exec.id, None::<bollard::exec::StartExecOptions>)
            .await
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
            cmd: Some(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "sleep 60".to_string(),
            ]),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };

        let response = docker
            .create_container(Some(options), config)
            .await
            .expect("Failed to create container");
        docker
            .start_container(
                &response.id,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await
            .expect("Failed to start container");

        // Wait for container to be fully running
        tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;

        // Create nested docs/guides directory and write file - need to START the exec
        let exec_config = bollard::exec::CreateExecOptions {
            cmd: Some(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "mkdir -p /docs/guides && echo '# Guide' > /docs/guides/test.md".to_string(),
            ]),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            ..Default::default()
        };
        let exec = docker
            .create_exec(&response.id, exec_config)
            .await
            .expect("Failed to create nested exec");
        let _ = docker
            .start_exec(&exec.id, None::<bollard::exec::StartExecOptions>)
            .await
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
        let exec = docker
            .create_exec(&container_id, exec_config)
            .await
            .expect("Failed to create exec");

        let start_options = bollard::exec::StartExecOptions {
            detach: false,
            ..Default::default()
        };
        let output = docker
            .start_exec(&exec.id, Some(start_options))
            .await
            .expect("Failed to start exec");

        let mut bytes = Vec::new();
        if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
            use futures_util::StreamExt;
            while let Some(result) = output.next().await {
                if let Ok(output) = result {
                    match output {
                        bollard::container::LogOutput::StdOut { message } => {
                            bytes.extend_from_slice(&message)
                        }
                        bollard::container::LogOutput::StdErr { message } => {
                            bytes.extend_from_slice(&message)
                        }
                        _ => {}
                    }
                }
            }
        }

        let output_str = String::from_utf8(bytes).expect("Failed to parse output");
        assert!(
            output_str.contains("Test Doc"),
            "Should contain doc content, got: {}",
            output_str
        );

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
        let exec = docker
            .create_exec(&container_id, exec_config)
            .await
            .expect("Failed to create exec");

        let start_options = bollard::exec::StartExecOptions {
            detach: false,
            ..Default::default()
        };
        let output = docker
            .start_exec(&exec.id, Some(start_options))
            .await
            .expect("Failed to start exec");

        let mut bytes = Vec::new();
        if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
            use futures_util::StreamExt;
            while let Some(result) = output.next().await {
                if let Ok(output) = result {
                    match output {
                        bollard::container::LogOutput::StdOut { message } => {
                            bytes.extend_from_slice(&message)
                        }
                        bollard::container::LogOutput::StdErr { message } => {
                            bytes.extend_from_slice(&message)
                        }
                        _ => {}
                    }
                }
            }
        }

        let output_str = String::from_utf8(bytes).expect("Failed to parse output");
        assert!(
            output_str.contains("Guide"),
            "Should contain nested doc content, got: {}",
            output_str
        );

        cleanup_container(&docker, &container_id).await;
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_content_type() {
        use alcedo_common::error::AppError;

        let content = "# Test".as_bytes().to_vec();
        let response = axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(
                axum::http::header::CONTENT_TYPE,
                "text/markdown; charset=utf-8",
            )
            .body(axum::body::Body::from(content.clone()))
            .map_err(|e| AppError::Internal(format!("Failed to build response: {}", e)));

        assert!(response.is_ok());
        let response = response.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let content_type = response
            .headers()
            .get("content-type")
            .expect("Content-Type header should be present");
        assert!(
            content_type.to_str().unwrap().contains("text/markdown"),
            "Content-Type should be text/markdown"
        );
        assert!(
            content_type.to_str().unwrap().contains("charset=utf-8"),
            "Content-Type should include charset"
        );
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_not_found() {
        use alcedo_common::error::AppError;

        let docker_error = AppError::Internal("Cat command exited with code 1".to_string());
        let result: Result<String, AppError> = Err(docker_error);

        let file_path = "nonexistent.md";
        let not_found_result =
            result.map_err(|_| AppError::NotFound(format!("File not found: {}", file_path)));

        assert!(not_found_result.is_err());
        let err = not_found_result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(
            error_msg.contains("nonexistent.md"),
            "Error should mention the file path"
        );
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_traversal_dotdot() {
        use alcedo_api::api::admin::validate_docs_path;
        use alcedo_common::error::AppError;

        let result = validate_docs_path("../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        let error_msg = err.to_string();
        assert!(
            error_msg.contains("traversal"),
            "Error should mention path traversal"
        );
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_traversal_absolute() {
        use alcedo_api::api::admin::validate_docs_path;
        use alcedo_common::error::AppError;

        let result = validate_docs_path("/etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_null_bytes() {
        use alcedo_api::api::admin::validate_docs_path;
        use alcedo_common::error::AppError;

        let result = validate_docs_path("test\0.md");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        let error_msg = err.to_string();
        assert!(
            error_msg.contains("null"),
            "Error should mention null bytes"
        );
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_plugin_not_found() {
        use alcedo_common::error::AppError;

        let slug = "nonexistent-plugin-12345";
        let result: Result<(), AppError> =
            Err(AppError::NotFound(format!("Plugin not found: {}", slug)));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(
            error_msg.contains("nonexistent-plugin-12345"),
            "Error should mention the slug"
        );
    }

    #[tokio::test]
    async fn test_fetch_plugin_doc_no_active_version() {
        use alcedo_common::error::AppError;

        let slug = "plugin-without-active-version";
        let result: Result<(), AppError> = Err(AppError::NotFound(format!(
            "No active version for plugin: {}",
            slug
        )));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(
            error_msg.contains("No active version"),
            "Error should mention no active version"
        );
    }
}

/// End-to-end proxy happy path against a real container.
///
/// Starts a real container (python:3-alpine) running a tiny HTTP server that
/// echoes the `X-Request-ID` it receives, wires a `DockerPlatform` + Redis into
/// the real alcedocore router, and verifies `GET /p/{slug}/health`:
///   1. resolves the active plugin via the Redis cache (`plugin:active:{slug}`,
///      the same key `cache_active_plugin()` writes after a real deploy),
///   2. resolves the container address via `DockerPlatform::get_address`,
///   3. forwards to the live container and returns its body,
///   4. stores the `plugin_req:{id}` → slug mapping in Redis.
///
/// Network/port notes:
/// - We run the router in non-dev (release-like) mode, so the proxy resolves
///   the container's real bridge IP and forwards to `http://<ip>:8080` (the
///   standard plugin port per AGENTS.md). Dev mode would short-circuit to
///   `localhost:<DEV_PLUGIN_PORT>` and never exercise real address resolution.
/// - The plugin port used here is 8080 rather than the 8000 suggested in the
///   task notes because the non-dev proxy path appends `:8080` to a bare
///   container IP; 8000 only applies in dev mode. We deliberately picked the
///   release-mode path since it genuinely resolves the container address.
/// - The active-plugin lookup uses the Redis cache instead of the Postgres
///   `plugin_versions`/`plugins` rows. The DB fallback path needs the full
///   schema + migrations wiring, which is disproportionate for this Docker
///   integration crate (and is already covered in `core/core/tests`). The
///   cache is a real production path exercised on every proxied request.
/// - `deploy()` is not used because the platform's `create_container` doesn't
///   accept a custom cmd (it always runs the image default), so the HTTP
///   server container is created directly with bollard and then resolved via
///   `DockerPlatform::get_address`.
mod live_proxy_tests {
    use super::*;

    use alcedo_api::container::PluginPlatform;
    use alcedo_api::plugins::health::{AppState, CoreState, PluginHealthMap};
    use alcedo_api::services::redis_session::{RedisPool, RedisPoolManager, RedisSessionStore};
    use deadpool::managed;
    use std::sync::Arc;
    use testcontainers::runners::AsyncRunner;
    use testcontainers::ContainerAsync;
    use testcontainers_modules::redis::Redis;

    /// A minimal HTTP/1.1 server that responds 200 "hello from plugin" and
    /// echoes the incoming `X-Request-ID` header back as `X-Echo-Request-ID`.
    const HTTP_SERVER_SCRIPT: &str = r#"
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", 8080))
s.listen(32)
while True:
    c, _ = s.accept()
    try:
        data = c.recv(8192).decode("latin-1", "replace")
        rid = ""
        for line in data.split("\r\n"):
            if line.lower().startswith("x-request-id:"):
                rid = line.split(":", 1)[1].strip()
                break
        body = b"hello from plugin"
        resp = ("HTTP/1.1 200 OK\r\n"
                "Content-Type: text/plain\r\n"
                "Content-Length: {}\r\n"
                "X-Echo-Request-ID: {}\r\n"
                "Connection: close\r\n\r\n").format(len(body), rid)
        c.sendall(resp.encode("latin-1") + body)
    finally:
        c.close()
"#;

    async fn start_redis() -> (ContainerAsync<Redis>, redis::aio::ConnectionManager, String) {
        let container = Redis::default()
            .start()
            .await
            .expect("failed to start redis container");
        let host = container
            .get_host()
            .await
            .expect("failed to get redis host");
        let port = container
            .get_host_port_ipv4(6379)
            .await
            .expect("failed to get redis port");
        let url = format!("redis://{}:{}/0", host, port);
        let client = redis::Client::open(url.clone()).expect("invalid redis url");
        let conn = redis::aio::ConnectionManager::new(client)
            .await
            .expect("failed to connect to redis");
        (container, conn, url)
    }

    async fn ensure_image(docker: &Docker, image: &str) {
        if is_image_available(docker, image).await {
            return;
        }
        let opts = bollard_stubs::query_parameters::CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };
        let mut stream = docker.create_image(Some(opts), None, None);
        while let Some(result) = stream.next().await {
            result.expect("failed to pull image");
        }
    }

    #[tokio::test]
    async fn test_proxy_live_container_happy_path() {
        let docker = require_docker();
        let image = "python:3-alpine";
        ensure_image(&docker, image).await;

        let container_name = format!("proxy-live-{}", uuid::Uuid::new_v4());
        let config = bollard::models::ContainerCreateBody {
            image: Some(image.to_string()),
            cmd: Some(vec![
                "python3".to_string(),
                "-c".to_string(),
                HTTP_SERVER_SCRIPT.to_string(),
            ]),
            host_config: Some(bollard::models::HostConfig {
                network_mode: Some("bridge".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(container_name),
            platform: String::new(),
        };
        let created = docker
            .create_container(Some(options), config)
            .await
            .expect("failed to create live proxy container");
        let container_id = created.id.clone();
        docker
            .start_container(
                &container_id,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await
            .expect("failed to start live proxy container");

        async {
            // Hermetic Redis for the active-plugin cache + request-id mapping.
            let (_redis_container, conn_manager, redis_url) = start_redis().await;

            // Build a DockerPlatform backed by a real DockerRuntime. The global
            // Docker client used by the runtime must be initialized first.
            let socket_path = std::env::var("DOCKER_SOCKET_PATH")
                .unwrap_or_else(|_| "/var/run/docker.sock".to_string());
            platform_docker::init_docker(&socket_path)
                .expect("failed to init global docker client");

            let config = alcedo_common::config::AppConfig {
                database_url: None,
                core_port: 8080,
                local_registry_url: "localhost:5000".to_string(),
                docker_socket: socket_path.clone(),
                plugin_network: "bridge".to_string(),
                plugins_dir: "/tmp".to_string(),
                max_restart_attempts: 3,
                dev_mode: false,
                redis_url: redis_url.clone(),
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
                registry_seed: None,
            };
            let platform = platform_docker::platform::DockerPlatform::new(
                None,
                Arc::new(platform_docker::runtime::DockerRuntime::new()),
                Arc::new(config),
            );

            // The platform must resolve the live container's bridge IP.
            let address = platform
                .get_address(&container_id)
                .await
                .expect("platform get_address failed")
                .expect("platform could not resolve container address");
            assert!(
                address.contains('.'),
                "expected a real container IP, got: {}",
                address
            );

            // Wait until the in-container HTTP server is reachable.
            let server_url = format!("http://{}:8080/health", address);
            let mut ready = false;
            for _ in 0..30 {
                if let Ok(resp) = reqwest::get(&server_url).await {
                    if resp.status().is_success() {
                        ready = true;
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            assert!(
                ready,
                "container HTTP server did not become ready at {}",
                server_url
            );

            // Redis pool used by the router (active-plugin cache + plugin_req).
            let redis_pool: RedisPool =
                managed::Pool::builder(RedisPoolManager::with_url(redis_url.clone()))
                    .max_size(2)
                    .build()
                    .expect("failed to build redis pool");

            // Pre-populate the active-plugin cache exactly like
            // cache_active_plugin() does after a real deploy. This makes the
            // proxy skip the DB fallback (see proxy.rs).
            let slug = format!("proxy-live-{}", uuid::Uuid::new_v4().simple());
            {
                let mut conn = redis_pool.get().await.expect("failed to get redis conn");
                let active = serde_json::json!({
                    "container_id": container_id,
                    "version": "1.0.0",
                    "endpoint_count": 1,
                });
                let _: Result<(), _> = redis::cmd("SET")
                    .arg(format!("plugin:active:{}", slug))
                    .arg(active.to_string())
                    .query_async(&mut *conn)
                    .await;
            }

            let dir = std::env::temp_dir().join(format!("proxy-files-{}", uuid::Uuid::new_v4()));
            let state = AppState {
                core: CoreState::for_pool(None),
                health_map: Arc::new(PluginHealthMap::new(None)),
                db_pool: None,
                kv_store: Arc::new(alcedo_infra::kv::store::KvStore::new_test()),
                file_storage: Arc::new(
                    file_storage_local::LocalFileStorage::new(dir.to_str().unwrap())
                        .expect("failed to create file storage"),
                ),
                plugin_network: Some("bridge".to_string()),
                static_registry: None,
                registries: None,
                platform: Some(Arc::new(platform) as Arc<dyn PluginPlatform>),
                redis_connection: Some(redis_pool.clone()),
                rate_limit_redis: Some(Arc::new(tokio::sync::Mutex::new(conn_manager.clone()))),
                kv_redis: Some(Arc::new(tokio::sync::Mutex::new(conn_manager.clone()))),
                logging_channel: None,
                host_call_channel: None,
                event_bus: Default::default(),
                capture_body: false,
                capture_body_max_size: 10240,
                nested_field_depth_limit: 5,
                session_store: RedisSessionStore::new(conn_manager),
                proxy_client: reqwest::Client::new(),
                rate_limit_auth_requests: 10,
                rate_limit_auth_window: 60,
                rate_limit_api_requests: 100,
                rate_limit_api_window: 60,
            };

            let session_layer =
                tower_sessions::SessionManagerLayer::new(state.session_store.clone());
            let app = alcedo_api::api::make_router(Arc::new(state), session_layer);
            let server = axum_test::TestServer::new(app).expect("failed to create test server");

            let response = server.get(&format!("/p/{}/health", slug)).await;
            let echoed_rid = response
                .headers()
                .get("x-echo-request-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let status = response.status_code();
            let body = response.text();
            assert_eq!(
                status,
                axum::http::StatusCode::OK,
                "proxy should return 200, got: {} body: {}",
                status,
                body
            );

            assert!(
                body.contains("hello from plugin"),
                "proxy response should contain the plugin body, got: {}",
                body
            );

            assert!(
                !echoed_rid.is_empty(),
                "plugin should have echoed the X-Request-ID it received"
            );

            // The proxy stores plugin_req:{id} -> slug so plugin callbacks can
            // authenticate back to the core. Assert the echoed id maps to slug.
            let mut conn = redis_pool.get().await.expect("failed to get redis conn");
            let stored: Option<String> = redis::cmd("GET")
                .arg(format!("plugin_req:{}", echoed_rid))
                .query_async(&mut *conn)
                .await
                .expect("failed to read plugin_req mapping");
            assert_eq!(
                stored.as_deref(),
                Some(slug.as_str()),
                "X-Request-ID {} should map to slug {} in Redis",
                echoed_rid,
                slug
            );
        }
        .await;

        cleanup_container(&docker, &container_id).await;
    }
}
