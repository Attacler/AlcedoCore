use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use mime_guess::MimeGuess;
use std::path::Path;
use std::sync::Arc;

use crate::db::queries::PluginVersion;
use crate::db::queries::SystemSetting;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;

fn reject_path_traversal(path: &str) -> Result<(), AppError> {
    if path.contains("..") {
        return Err(AppError::BadRequest("Path traversal detected".to_string()));
    }
    if path.contains('\0') {
        return Err(AppError::BadRequest("Invalid path".to_string()));
    }
    Ok(())
}

pub async fn serve_static_file(
    State(state): State<Arc<PluginAppState>>,
    axum::extract::Path(path_info): axum::extract::Path<super::SlugPath>,
) -> Response {
    // Reject path traversal attempts
    if let Err(e) = reject_path_traversal(&path_info.path) {
        tracing::warn!("[STATIC] Path traversal blocked: {}", path_info.path);
        return e.into_response();
    }

    let db_pool = match state.db_pool.as_ref() {
        Some(pool) => pool,
        None => {
            tracing::error!("[STATIC] No database pool configured");
            return AppError::Internal("Database not configured".to_string()).into_response();
        }
    };

    let is_static = match crate::db::queries::Plugin::find_by_slug(db_pool, &path_info.slug).await {
        Ok(Some(p)) => p.plugin_type == "static",
        Ok(None) => false,
        Err(_) => false,
    };

    let plugins_dir_str = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
    let plugins_dir = Path::new(&plugins_dir_str);

    let file_bytes = if is_static {
        let local_path = plugins_dir
            .join(&path_info.slug)
            .join("public")
            .join(&path_info.path);
        tracing::info!(
            "[STATIC] Serving static plugin file from: {}",
            local_path.display()
        );
        match std::fs::read(&local_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(
                    "[STATIC] Static file not found in public/: {} - {}",
                    local_path.display(),
                    e
                );
                // Fallback: check pages/assets/ for admin-style plugins
                // path like "assets/index-Cq01UYkh.css" -> pages/assets/index-Cq01UYkh.css
                // path like "index-Cq01UYkh.css" -> pages/assets/index-Cq01UYkh.css
                let pages_base = plugins_dir
                    .join(&path_info.slug)
                    .join("pages")
                    .join("assets");
                let pages_path = if path_info.path.starts_with("assets/") {
                    pages_base.join(
                        path_info
                            .path
                            .strip_prefix("assets/")
                            .unwrap_or(&path_info.path),
                    )
                } else {
                    pages_base.join(&path_info.path)
                };
                tracing::info!(
                    "[STATIC] Trying pages/assets/ fallback: {}",
                    pages_path.display()
                );
                match std::fs::read(&pages_path) {
                    Ok(bytes) => bytes,
                    Err(_e2) => {
                        tracing::warn!(
                            "[STATIC] Static file not found in pages/assets/ either: {}",
                            pages_path.display()
                        );
                        return AppError::NotFound(format!("File not found: {}", path_info.path))
                            .into_response();
                    }
                }
            }
        }
    } else {
        let active_version = match PluginVersion::find_active(db_pool, &path_info.slug).await {
            Ok(Some(v)) => v,
            Ok(None) => {
                tracing::warn!("[STATIC] No active version found for {}", path_info.slug);
                return AppError::NotFound(format!(
                    "No active version found for plugin: {}",
                    path_info.slug
                ))
                .into_response();
            }
            Err(e) => {
                tracing::error!("[STATIC] Database error: {}", e);
                return AppError::Internal(format!("Database error: {}", e)).into_response();
            }
        };

        match try_read_from_mount(
            &active_version.slug,
            &active_version.version,
            &path_info.path,
        ) {
            Ok(Some(bytes)) => {
                tracing::info!("[STATIC] Mount hit: /public/{}", path_info.path);
                bytes
            }
            Ok(None) | Err(_) => {
                // Fallback: check extracted public directory on filesystem
                // (populated by deploy handler via extract_from_image).
                let plugins_dir_str =
                    std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
                let fs_path = std::path::Path::new(&plugins_dir_str)
                    .join(&path_info.slug)
                    .join("public")
                    .join(&path_info.path);
                match std::fs::read(&fs_path) {
                    Ok(bytes) => {
                        tracing::info!("[STATIC] Filesystem hit: {}", fs_path.display());
                        let mime = if path_info.path.contains('.') {
                            MimeGuess::from_path(&path_info.path).first_or_octet_stream()
                        } else {
                            mime_guess::mime::APPLICATION_OCTET_STREAM
                        };
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", mime.as_ref())
                            .header("Content-Length", bytes.len())
                            .body(axum::body::Body::from(bytes))
                            .unwrap_or_else(|_| {
                                AppError::Internal("Failed to build response".to_string())
                                    .into_response()
                            });
                    }
                    Err(_) => {
                        tracing::info!(
                            "[STATIC] Filesystem miss, trying exec: /public/{}",
                            path_info.path
                        );
                    }
                }
                let container_id = match &active_version.container_id {
                    Some(id) => id.clone(),
                    None => {
                        tracing::error!("[STATIC] No container ID for active version");
                        return AppError::Internal(
                            "No container ID for active version".to_string(),
                        )
                        .into_response();
                    }
                };
                let file_path = format!("/public/{}", path_info.path);
                let file_result = match state.platform.as_ref() {
                    Some(platform) => platform.read_file(&container_id, &file_path).await,
                    None => Err(AppError::Internal("Platform not configured".to_string())),
                };
                match file_result {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        tracing::warn!("[STATIC] File not found: {}", file_path);
                        return AppError::NotFound(format!("File not found: {}", path_info.path))
                            .into_response();
                    }
                }
            }
        }
    };

    let mime = if path_info.path.contains('.') {
        MimeGuess::from_path(&path_info.path).first_or_octet_stream()
    } else {
        mime_guess::mime::APPLICATION_OCTET_STREAM
    };

    tracing::info!(
        "[STATIC] Serving file ({} bytes, mime: {})",
        file_bytes.len(),
        mime
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", mime.as_ref())
        .header("Content-Length", file_bytes.len())
        .body(axum::body::Body::from(file_bytes))
        .unwrap_or_else(|_| {
            AppError::Internal("Failed to build response".to_string()).into_response()
        })
}

async fn try_catch_all_proxy(state: &Arc<PluginAppState>, path: &str) -> Option<Response> {
    let db_pool = state.db_pool.as_ref()?;
    let setting = SystemSetting::find_by_key(db_pool, "catch_all_plugin_slug")
        .await
        .ok()??;
    let slug = setting.value.as_str()?.to_string();
    if slug.is_empty() {
        return None;
    }

    let request = axum::http::Request::builder()
        .uri(if path.is_empty() {
            format!("/p/{}", slug)
        } else {
            format!("/p/{}/{}", slug, path)
        })
        .body(axum::body::Body::empty())
        .ok()?;

    match super::proxy::proxy_handler(
        axum::extract::State(state.clone()),
        axum::extract::Path(super::SlugPath {
            slug: slug.clone(),
            path: path.to_string(),
        }),
        request,
    )
    .await
    {
        Ok(r) => {
            let resp = r.into_response();
            let (parts, body) = resp.into_parts();
            let content_type = parts
                .headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            if content_type.contains("text/html") {
                let body_bytes = axum::body::to_bytes(body, 10_000_000).await.ok()?;
                let body_str = String::from_utf8_lossy(&body_bytes);
                let replaced = body_str.replace(&format!("/{}/", slug), "/");
                let new_body = replaced.as_bytes().to_vec();
                let mut builder = axum::response::Response::builder().status(parts.status);
                for (name, value) in parts.headers.iter() {
                    if name.as_str() != "content-length" {
                        builder = builder.header(name.as_str(), value);
                    }
                }
                builder = builder.header("Content-Length", new_body.len());
                return Some(
                    builder
                        .body(axum::body::Body::from(new_body))
                        .unwrap_or_else(|_| {
                            AppError::Internal("Body rewrite failed".to_string()).into_response()
                        }),
                );
            }

            Some(Response::from_parts(parts, body))
        }
        Err(_) => None,
    }
}

pub async fn serve_index_or_static(
    State(state): State<Arc<PluginAppState>>,
    axum::extract::Path(slug_info): axum::extract::Path<super::SlugPath>,
) -> Response {
    // Reject path traversal attempts
    if let Err(e) = reject_path_traversal(&slug_info.path) {
        tracing::warn!("[STATIC] Path traversal blocked: {}", slug_info.path);
        return e.into_response();
    }

    let slug = slug_info.slug.clone();

    let plugins_dir_str = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
    let plugins_dir = Path::new(&plugins_dir_str);

    let manifest_path = plugins_dir.join(&slug).join("manifest.json");
    let is_static = manifest_path.exists()
        && match std::fs::read_to_string(&manifest_path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(manifest) => {
                    manifest.get("plugin_type").and_then(|v| v.as_str()) == Some("static")
                }
                Err(_) => false,
            },
            Err(_) => false,
        };

    let file_path = if slug_info.path.is_empty() || slug_info.path == "/" {
        "index.html".to_string()
    } else {
        slug_info.path.clone()
    };

    let file_bytes = if is_static {
        let local_path = plugins_dir.join(&slug).join("public").join(&file_path);
        tracing::info!(
            "[STATIC] Serving static plugin index from: {}",
            local_path.display()
        );
        match std::fs::read(&local_path) {
            Ok(bytes) => bytes,
            Err(_) => {
                // If specific path requested (not index), try pages/assets/ fallback before giving up
                if file_path != "index.html" && !file_path.is_empty() {
                    // Try pages/assets/ for admin-style plugins (path like "assets/index-Cq01UYkh.css")
                    let pages_base = plugins_dir.join(&slug).join("pages").join("assets");
                    let pages_path = if file_path.starts_with("assets/") {
                        pages_base.join(file_path.strip_prefix("assets/").unwrap_or(&file_path))
                    } else {
                        pages_base.join(&file_path)
                    };
                    tracing::info!(
                        "[STATIC] Trying pages/assets/ fallback: {}",
                        pages_path.display()
                    );
                    match std::fs::read(&pages_path) {
                        Ok(bytes) => bytes,
                        Err(_e2) => {
                            tracing::warn!("[STATIC] File not found in pages/assets/ either");
                            return AppError::NotFound(format!("File not found: {}", file_path))
                                .into_response();
                        }
                    }
                } else {
                    // For index.html, try pages/index.html fallback
                    tracing::info!(
                        "[STATIC] Static file not found: {}, trying index.html",
                        local_path.display()
                    );
                    let index_path = plugins_dir.join(&slug).join("public").join("index.html");
                    match std::fs::read(&index_path) {
                        Ok(bytes) => bytes,
                        Err(_e) => {
                            tracing::info!(
                                "[STATIC] public/index.html not found, trying pages/index.html"
                            );
                            let pages_index_path =
                                plugins_dir.join(&slug).join("pages").join("index.html");
                            match std::fs::read(&pages_index_path) {
                                Ok(bytes) => bytes,
                                Err(_e2) => {
                                    tracing::warn!("[STATIC] Both public/index.html and pages/index.html not found");
                                    return AppError::NotFound(format!(
                                        "File not found: {}",
                                        file_path
                                    ))
                                    .into_response();
                                }
                            }
                        }
                    }
                }
            }
        }
    } else if state.db_pool.is_some() {
        let db_pool = state.db_pool.as_ref().unwrap();

        let is_static_from_db = match crate::db::queries::Plugin::find_by_slug(db_pool, &slug).await
        {
            Ok(Some(p)) => p.plugin_type == "static",
            Ok(None) => false,
            Err(_) => false,
        };

        if is_static_from_db {
            let local_path = plugins_dir.join(&slug).join("public").join(&file_path);
            tracing::info!(
                "[STATIC] Serving static plugin from DB check: {}",
                local_path.display()
            );
            match std::fs::read(&local_path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    tracing::info!(
                        "[STATIC] Static file not found: {}, trying index.html",
                        local_path.display()
                    );
                    let index_path = plugins_dir.join(&slug).join("public").join("index.html");
                    match std::fs::read(&index_path) {
                        Ok(bytes) => bytes,
                        Err(_e) => {
                            // Fallback: check pages/index.html for admin-style plugins
                            tracing::info!(
                                "[STATIC] public/index.html not found, trying pages/index.html"
                            );
                            let pages_index_path =
                                plugins_dir.join(&slug).join("pages").join("index.html");
                            match std::fs::read(&pages_index_path) {
                                Ok(bytes) => bytes,
                                Err(_e2) => {
                                    tracing::warn!("[STATIC] Both public/index.html and pages/index.html not found");
                                    return AppError::NotFound(format!(
                                        "File not found: {}",
                                        file_path
                                    ))
                                    .into_response();
                                }
                            }
                        }
                    }
                }
            }
        } else {
            let active_version = match PluginVersion::find_active(db_pool, &slug).await {
                Ok(Some(v)) => v,
                Ok(None) => {
                    let full_catch_path = if slug_info.path.is_empty() {
                        slug.clone()
                    } else {
                        format!("{}/{}", slug, slug_info.path)
                    };
                    if let Some(response) = try_catch_all_proxy(&state, &full_catch_path).await {
                        return response;
                    }
                    tracing::warn!("[STATIC] No active version found for {}", slug);
                    return AppError::NotFound(format!(
                        "No active version found for plugin: {}",
                        slug
                    ))
                    .into_response();
                }
                Err(e) => {
                    tracing::error!("[STATIC] Database error: {}", e);
                    return AppError::Internal(format!("Database error: {}", e)).into_response();
                }
            };

            match try_read_from_mount(&slug, &active_version.version, &file_path) {
                Ok(Some(bytes)) => {
                    tracing::info!("[STATIC] Mount hit: /public/{}", file_path);
                    bytes
                }
                Ok(None) | Err(_) => {
                    // Fallback: check extracted public directory on filesystem
                    let plugins_dir_str =
                        std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
                    let fs_path = std::path::Path::new(&plugins_dir_str)
                        .join(&slug)
                        .join("public")
                        .join(&file_path);
                    match std::fs::read(&fs_path) {
                        Ok(bytes) => {
                            tracing::info!("[STATIC] Filesystem hit: {}", fs_path.display());
                            let mime = if file_path.contains('.') {
                                mime_guess::MimeGuess::from_path(&file_path).first_or_octet_stream()
                            } else {
                                mime_guess::mime::APPLICATION_OCTET_STREAM
                            };
                            return Response::builder()
                                .status(StatusCode::OK)
                                .header("Content-Type", mime.as_ref())
                                .header("Content-Length", bytes.len())
                                .body(axum::body::Body::from(bytes))
                                .unwrap_or_else(|_| {
                                    AppError::Internal("Failed to build response".to_string())
                                        .into_response()
                                });
                        }
                        Err(_) => {
                            tracing::info!("[STATIC] Mount miss, returning 404");
                            return AppError::NotFound(format!("File not found: {}", file_path))
                                .into_response();
                        }
                    }
                }
            }
        }
    } else {
        return AppError::NotFound(format!("Plugin {} not found (no database)", slug))
            .into_response();
    };

    let mime = if file_path.contains('.') {
        MimeGuess::from_path(&file_path).first_or_octet_stream()
    } else {
        mime_guess::mime::APPLICATION_OCTET_STREAM
    };

    tracing::info!(
        "[STATIC] Serving {} ({} bytes)",
        file_path,
        file_bytes.len()
    );
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", mime.as_ref())
        .header("Content-Length", file_bytes.len())
        .body(axum::body::Body::from(file_bytes))
        .unwrap_or_else(|_| {
            AppError::Internal("Failed to build response".to_string()).into_response()
        })
}

fn try_read_from_mount(
    slug: &str,
    version: &str,
    path: &str,
) -> Result<Option<Vec<u8>>, std::io::Error> {
    // Reject path traversal
    if path.contains("..") || path.contains('\0') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Path traversal detected",
        ));
    }
    let mount_base =
        std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/var/lib/plugin-public".to_string());

    let file_path = Path::new(&mount_base)
        .join(slug)
        .join(version)
        .join("public")
        .join(path);

    if file_path.exists() {
        match std::fs::read(&file_path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) => {
                tracing::warn!(
                    "[STATIC] Mount file exists but read failed: {} - {}",
                    file_path.display(),
                    e
                );
                Err(e)
            }
        }
    } else {
        Ok(None)
    }
}
