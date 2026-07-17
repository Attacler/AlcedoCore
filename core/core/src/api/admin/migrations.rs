use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use std::sync::Arc;

use crate::api::permission_check;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;
use crate::db::plugin_migrations::{plugin_schema_name, PluginMigrationEngine};
use crate::db::schema::get_table_schemas;
use crate::db::queries::PluginVersion;
use crate::services::scopes::{check_entity_scope, ScopeSource};

pub async fn get_plugin_schema(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let schema_name = plugin_schema_name(&slug);

    let schema_exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)"#,
    )
    .bind(&schema_name)
    .fetch_one(db_pool)
    .await?;

    if !schema_exists {
        return Ok(Json(serde_json::json!({
            "plugin_name": slug,
            "schema_name": schema_name,
            "tables": []
        })));
    }

    let tables = get_table_schemas(db_pool, &schema_name).await?;

    Ok(Json(serde_json::json!({
        "plugin_name": slug,
        "schema_name": schema_name,
        "tables": tables
    })))
}

pub fn plugin_file_dir(slug: &str, version: &str, subdir: &str) -> Option<std::path::PathBuf> {
    if let Ok(mount) = std::env::var("PLUGIN_PUBLIC_MOUNTS") {
        let path = std::path::Path::new(&mount).join(slug).join(version).join(subdir);
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(dir) = std::env::var("PLUGINS_DIR") {
        let path = std::path::Path::new(&dir).join(slug).join(subdir);
        if path.exists() {
            return Some(path);
        }
        if subdir == "migrations" {
            let old_path = std::path::Path::new(&dir).join("plugin-migrations").join(slug);
            if old_path.exists() {
                return Some(old_path);
            }
        }
    }
    None
}

pub async fn list_migrations(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let version = PluginVersion::find_active(db_pool, &slug).await?
        .map(|v| v.version);
    let migrations_dir = match version {
        Some(ref ver) => crate::api::admin::plugin_file_dir(&slug, ver, "migrations")
            .unwrap_or_else(|| {
                let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
                std::path::Path::new(&dir).join("plugin-migrations").join(&slug)
            }),
        None => {
            let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
            std::path::Path::new(&dir).join("plugin-migrations").join(&slug)
        }
    };

    if !migrations_dir.exists() {
        return Ok(Json(vec![]));
    }

    let engine = PluginMigrationEngine::new(
        db_pool.clone(),
        migrations_dir,
        &slug,
    );

    let statuses = engine.get_migration_status().await?;

    let result: Vec<serde_json::Value> = statuses.into_iter().map(|s| {
        serde_json::json!({
            "version": s.version,
            "name": s.name,
            "filename": s.filename,
            "status": if s.applied { "applied" } else { "pending" },
            "applied": s.applied,
            "applied_at": s.applied_at.map(|dt| dt.to_rfc3339()),
            "has_down": s.has_down,
            "sql": s.sql,
            "schema": engine.schema_name(),
        })
    }).collect();

    Ok(Json(result))
}

pub async fn run_migration(
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    check_entity_scope(db_pool, ScopeSource::Plugin { slug: &slug }, "db.migrate").await?;

    let version = PluginVersion::find_active(db_pool, &slug).await?
        .map(|v| v.version);
    let migrations_dir = match version {
        Some(ref ver) => crate::api::admin::plugin_file_dir(&slug, ver, "migrations")
            .unwrap_or_else(|| {
                let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
                std::path::Path::new(&dir).join("plugin-migrations").join(&slug)
            }),
        None => {
            let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
            std::path::Path::new(&dir).join("plugin-migrations").join(&slug)
        }
    };

    if !migrations_dir.exists() {
        return Err(AppError::NotFound(format!(
            "No migrations directory found for plugin {}",
            slug
        )));
    }

    let engine = PluginMigrationEngine::new(
        db_pool.clone(),
        migrations_dir,
        &slug,
    );

    match engine.run_migrations().await {
        Ok(ran) => {
            PluginVersion::update_status(db_pool, &slug, &version.unwrap_or_default(), "running").await?;
            Ok(Json(serde_json::json!({
                "success": ran.errors.is_empty(),
                "applied": ran.applied,
                "errors": ran.errors,
                "message": if ran.applied.is_empty() {
                    "No pending migrations to apply".to_string()
                } else {
                    format!("Applied {} migration(s)", ran.applied.len())
                }
            })))
        }
        Err(e) => {
            Err(AppError::Internal(format!("Failed to run migrations: {}", e)))
        }
    }
}

pub async fn rollback_migration(
    Path((slug, target_version)): Path<(String, String)>,
    State(state): State<Arc<PluginAppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db_pool.as_ref().ok_or_else(|| {
        AppError::BadRequest("Database not configured".to_string())
    })?;

    check_entity_scope(db_pool, ScopeSource::Plugin { slug: &slug }, "db.migrate").await?;

    let version = PluginVersion::find_active(db_pool, &slug).await?
        .map(|v| v.version);
    let migrations_dir = match version {
        Some(ref ver) => crate::api::admin::plugin_file_dir(&slug, ver, "migrations")
            .unwrap_or_else(|| {
                let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
                std::path::Path::new(&dir).join("plugin-migrations").join(&slug)
            }),
        None => {
            let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
            std::path::Path::new(&dir).join("plugin-migrations").join(&slug)
        }
    };

    if !migrations_dir.exists() {
        return Err(AppError::NotFound(format!(
            "No migrations directory found for plugin {}",
            slug
        )));
    }

    let engine = PluginMigrationEngine::new(
        db_pool.clone(),
        migrations_dir,
        &slug,
    );

    let rolled_back = engine.rollback_to(&target_version).await?;

    Ok(Json(serde_json::json!({
        "success": true,
        "rolled_back_versions": rolled_back,
        "message": if rolled_back.is_empty() {
            "Already at target version".to_string()
        } else {
            format!("Rolled back {} migration(s) to version {}", rolled_back.len(), target_version)
        },
        "target_version": target_version,
        "schema": engine.schema_name(),
    })))
}

pub async fn upload_migrations_handler(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
    Json(payload): Json<crate::api::admin::UploadMigrationsRequest>,
) -> Result<Json<crate::api::admin::UploadMigrationsResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
    let migrations_dir = std::path::Path::new(&plugins_dir)
        .join("plugin-migrations")
        .join(&slug);

    std::fs::create_dir_all(&migrations_dir).map_err(AppError::Io)?;

    let mut results = Vec::with_capacity(payload.files.len());

    for file in &payload.files {
        if file.filename.contains('/')
            || file.filename.contains('\\')
            || file.filename == "."
            || file.filename == ".."
        {
            results.push(crate::api::admin::UploadResult {
                filename: file.filename.clone(),
                status: "error".to_string(),
                error: Some("Invalid filename: contains path separator or directory reference".to_string()),
            });
            continue;
        }

        let file_path = migrations_dir.join(&file.filename);

        if file_path.exists() {
            results.push(crate::api::admin::UploadResult {
                filename: file.filename.clone(),
                status: "skipped".to_string(),
                error: Some("File already exists".to_string()),
            });
            continue;
        }

        match std::fs::write(&file_path, &file.content) {
            Ok(()) => {
                results.push(crate::api::admin::UploadResult {
                    filename: file.filename.clone(),
                    status: "created".to_string(),
                    error: None,
                });
            }
            Err(e) => {
                results.push(crate::api::admin::UploadResult {
                    filename: file.filename.clone(),
                    status: "error".to_string(),
                    error: Some(e.to_string()),
                });
            }
        }
    }

    Ok(Json(crate::api::admin::UploadMigrationsResponse { results }))
}
