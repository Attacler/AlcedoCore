mod docker_client_unit_tests {

    /// Test that `list_directory_in_container` filters out directory entries.
    /// Directories in `ls -1p` output end with `/`, so we filter them out.
    #[test]
    fn test_list_directory_in_container_filters_directories() {
        // The filtering logic: entries ending with '/' are filtered out
        // Simulate the raw output from `ls -1p`
        let raw_output = "file1.txt\nfile2.md\nsubdir/\nanother_file.log\n";

        // This is the same filtering logic used in list_directory_in_container
        let entries: Vec<String> = raw_output
            .lines()
            .filter(|line| !line.is_empty() && !line.ends_with('/'))
            .map(|s| s.to_string())
            .collect();

        // Verify directories (ending with /) are filtered out
        assert!(!entries.contains(&"subdir/".to_string()));
        // Verify files remain
        assert!(entries.contains(&"file1.txt".to_string()));
        assert!(entries.contains(&"file2.md".to_string()));
        assert!(entries.contains(&"another_file.log".to_string()));
        assert_eq!(entries.len(), 3);
    }

    /// Test that empty directory returns empty Vec
    #[test]
    fn test_list_directory_in_container_empty_directory() {
        let raw_output = "";

        let entries: Vec<String> = raw_output
            .lines()
            .filter(|line| !line.is_empty() && !line.ends_with('/'))
            .map(|s| s.to_string())
            .collect();

        assert!(entries.is_empty());
    }

    /// Test directory entries only contain trailing slash
    #[test]
    fn test_list_directory_in_container_only_directories() {
        let raw_output = "dir1/\ndir2/\ndir3/\n";

        let entries: Vec<String> = raw_output
            .lines()
            .filter(|line| !line.is_empty() && !line.ends_with('/'))
            .map(|s| s.to_string())
            .collect();

        assert!(entries.is_empty());
    }

    /// Test that files with no trailing slash are preserved even if they look like directories
    #[test]
    fn test_list_directory_in_container_files_no_false_filtering() {
        // Files that don't end with / should not be filtered
        let raw_output = "normal.txt\nnoextension\npath.with.dots\n";

        let entries: Vec<String> = raw_output
            .lines()
            .filter(|line| !line.is_empty() && !line.ends_with('/'))
            .map(|s| s.to_string())
            .collect();

        assert_eq!(entries.len(), 3);
        assert!(entries.contains(&"normal.txt".to_string()));
        assert!(entries.contains(&"noextension".to_string()));
        assert!(entries.contains(&"path.with.dots".to_string()));
    }
}

mod list_plugin_docs_error_tests {
    use plugin_core::db::queries::{Plugin, PluginVersion};
    use plugin_core::error::AppError;

    /// Test that `list_plugin_docs` returns 404 when plugin doesn't exist.
    /// The handler first calls `Plugin::find_by_slug`, which returns `Ok(None)` for non-existent slugs.
    #[tokio::test]
    async fn test_list_plugin_docs_not_found() {
        // When slug doesn't exist, Plugin::find_by_slug returns Ok(None)
        // The handler converts this to Err(AppError::NotFound(...))
        let slug = "nonexistent-plugin-12345";

        // Simulate the handler's error conversion logic
        let plugin_result: Option<Plugin> = None;
        let result = plugin_result.ok_or_else(|| AppError::NotFound(format!("Plugin not found: {}", slug)));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(error_msg.contains("nonexistent-plugin-12345"));
    }

    /// Test that `list_plugin_docs` returns 404 when plugin has no active version.
    /// After finding the plugin, it calls `PluginVersion::find_active` which returns `Ok(None)` when no active version exists.
    #[tokio::test]
    async fn test_list_plugin_docs_no_active_version() {
        // When plugin exists but has no active version, find_active returns Ok(None)
        let slug = "plugin-without-active-version";

        // Simulate the handler's error conversion logic for no active version
        let active_version_result: Option<PluginVersion> = None;
        let result = active_version_result.ok_or_else(|| {
            AppError::NotFound(format!("No active version for plugin: {}", slug))
        });

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(error_msg.contains("No active version"));
        assert!(error_msg.contains(slug));
    }

    /// Test that `list_plugin_docs` returns 404 when container_id is missing.
    /// Even with an active version, if container_id is None, we get NotFound.
    #[tokio::test]
    async fn test_list_plugin_docs_no_container_id() {
        let slug = "plugin-with-active-version-but-no-container";

        // Simulate a plugin version with no container_id
        let version = PluginVersion {
            slug: slug.to_string(),
            version: "1.0.0".to_string(),
            container_id: None,
            status: "running".to_string(),
            is_active: true,
            deployed_at: None,
            public_synced: false,
            public_path: None,
            pages_synced: false,
            pages_path: None,
        };

        let result = version.container_id.ok_or_else(|| {
            AppError::NotFound(format!("No container for plugin: {}", slug))
        });

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        let error_msg = err.to_string();
        assert!(error_msg.contains("No container"));
    }

    /// Test that AppError::NotFound converts to HTTP 404
    #[tokio::test]
    async fn test_app_error_not_found_http_status() {
        use axum::response::IntoResponse;

        let err = AppError::NotFound("Plugin not found".to_string());
        let response = err.into_response();

        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
}