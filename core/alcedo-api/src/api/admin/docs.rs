use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
    Json,
};
use std::sync::Arc;
use walkdir::WalkDir;

use crate::api::permission_check;
use crate::api::responses::ResponseEnvelope;
use crate::db::queries::{Plugin, PluginVersion};
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;

pub async fn list_plugin_docs(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<ResponseEnvelope<crate::api::admin::PluginDocsList>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let mount_base =
        std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/var/lib/plugin-public".to_string());
    if let Ok(Some(active_version)) = PluginVersion::find_active(db_pool, &slug).await {
        let version = &active_version.version;
        let docs_path = std::path::Path::new(&mount_base)
            .join(&slug)
            .join(version)
            .join("docs");

        if docs_path.exists() {
            let mut docs = Vec::new();
            let files = traverse_directories_relative(&docs_path);

            println!("Dirs: {:?}", files);

            for (directory, files) in files {
                for file in files {
                    if directory.len() > 0 {
                        docs.push(crate::api::admin::DocEntry {
                            path: format!("{}/{}", directory, file),
                            size: 0,
                        });
                    } else {
                        docs.push(crate::api::admin::DocEntry {
                            path: file,
                            size: 0,
                        });
                    }
                }
            }
            // if let Ok(entries) = std::fs::read_dir(&docs_path) {
            //     for entry in entries.flatten() {
            //         if let Ok(file_type) = entry.file_type() {
            //             if file_type.is_file() {
            //                 docs.push(crate::api::admin::DocEntry {
            //                     path: entry.file_name().to_string_lossy().to_string(),
            //                     size: 0,
            //                 });
            //             }
            //         }
            //     }
            // }
            return Ok(Json(ResponseEnvelope::success(
                crate::api::admin::PluginDocsList {
                    plugin: slug.clone(),
                    docs,
                },
            )));
        }
    }

    Ok(Json(ResponseEnvelope::success(
        crate::api::admin::PluginDocsList {
            plugin: slug,
            docs: vec![],
        },
    )))
}

pub fn traverse_directories_relative(root: &std::path::Path) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();

    // First, handle the root directory itself
    if root.is_dir() {
        let file_names: Vec<String> = root
            .read_dir()
            .map(|dir_entries| {
                dir_entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map_or(false, |ft| ft.is_file()))
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();

        result.push(("".to_string(), file_names));
    }

    // Then, handle subdirectories recursively
    for entry in WalkDir::new(root) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // Skip errors (e.g., permission denied)
        };

        if entry.file_type().is_dir() {
            // Skip the root directory (already handled above)
            if entry.path() == root {
                continue;
            }

            // Get the directory path relative to the root
            let dir_path = entry
                .path()
                .strip_prefix(root)
                .unwrap_or_else(|_| entry.path());
            let dir_path_str = dir_path.to_string_lossy().into_owned();

            // Collect file names in this directory
            let file_names: Vec<String> = entry
                .path()
                .read_dir()
                .map(|dir_entries| {
                    dir_entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().map_or(false, |ft| ft.is_file()))
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();

            result.push((dir_path_str, file_names));
        }
    }

    result
}

fn render_docs_directory_markdown(dir_path: &str, entries: &[String]) -> String {
    let mut md = format!("# Documentation: {}\n\n", dir_path);
    md.push_str("## Files\n\n");
    for entry in entries {
        let display_name = entry.trim_end_matches('/');
        let entry_path = if dir_path == "docs" {
            display_name.to_string()
        } else {
            format!(
                "{}/{}",
                dir_path.strip_prefix("docs/").unwrap_or(dir_path),
                display_name
            )
        };
        md.push_str(&format!("- [{}](./{})\n", display_name, entry_path));
    }
    md.push_str("\n---\n*Directory listing via AlcedoCore docs API*\n");
    md
}

pub fn validate_docs_path(path: &str) -> Result<(), AppError> {
    if path.starts_with('/') {
        return Err(AppError::BadRequest(
            "Absolute paths not allowed".to_string(),
        ));
    }

    if path.contains('\0') {
        return Err(AppError::BadRequest("Path contains null bytes".to_string()));
    }

    if path.contains("..") {
        return Err(AppError::BadRequest(
            "Path traversal not allowed".to_string(),
        ));
    }

    Ok(())
}

pub async fn fetch_plugin_doc(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path((slug, path)): Path<(String, String)>,
) -> Result<Response, AppError> {
    validate_docs_path(&path)?;

    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let _plugin = Plugin::find_by_slug(db_pool, &slug)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Plugin not found: {}", slug)))?;

    let active_version = PluginVersion::find_active(db_pool, &slug)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("No active version for plugin: {}", slug)))?;

    let container_id = active_version
        .container_id
        .filter(|c| !c.is_empty())
        .ok_or_else(|| AppError::NotFound(format!("No container for plugin: {}", slug)))?;

    let normalized_path = path.trim_end_matches('/');
    let docs_subpath = format!("docs/{}", normalized_path);

    let mount_base =
        std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/var/lib/plugin-public".to_string());
    let version = &active_version.version;
    let file_path = std::path::Path::new(&mount_base)
        .join(&slug)
        .join(version)
        .join(&docs_subpath);
    if file_path.exists() {
        let build_md_response = |body: String| {
            axum::response::Response::builder()
                .status(axum::http::StatusCode::OK)
                .header(
                    axum::http::header::CONTENT_TYPE,
                    "text/markdown; charset=utf-8",
                )
                .body(axum::body::Body::from(body))
                .map_err(|e| AppError::Internal(format!("Failed to build response: {}", e)))
        };
        if path.ends_with('/') || normalized_path.is_empty() {
            if let Ok(entries) = std::fs::read_dir(&file_path) {
                let names: Vec<String> = entries
                    .flatten()
                    .filter_map(|e| e.file_name().to_string_lossy().to_string().into())
                    .collect();
                let md_content = render_docs_directory_markdown(&docs_subpath, &names);
                return build_md_response(md_content);
            }
        } else {
            match std::fs::read_to_string(&file_path) {
                Ok(content) => return build_md_response(content),
                Err(_) => {}
            }
        }
    }

    if let Some(ref platform) = state.platform {
        let build_md_response = |body: String| {
            axum::response::Response::builder()
                .status(axum::http::StatusCode::OK)
                .header(
                    axum::http::header::CONTENT_TYPE,
                    "text/markdown; charset=utf-8",
                )
                .body(axum::body::Body::from(body))
                .map_err(|e| AppError::Internal(format!("Failed to build response: {}", e)))
        };
        if path.ends_with('/') || normalized_path.is_empty() {
            match platform.list_directory(&container_id, &docs_subpath).await {
                Ok(entries) => {
                    let md_content = render_docs_directory_markdown(&docs_subpath, &entries);
                    return build_md_response(md_content);
                }
                Err(_) => return Err(AppError::NotFound("Docs directory not found".to_string())),
            }
        }
        match platform.read_file(&container_id, &docs_subpath).await {
            Ok(content) => {
                return build_md_response(String::from_utf8_lossy(&content).to_string());
            }
            Err(_) => {
                let alt = format!("/app/{}", docs_subpath);
                match platform.read_file(&container_id, &alt).await {
                    Ok(content) => {
                        return build_md_response(String::from_utf8_lossy(&content).to_string())
                    }
                    Err(_) => return Err(AppError::NotFound("Doc not found".to_string())),
                }
            }
        }
    }

    Err(AppError::NotFound("Doc not found".to_string()))
}
