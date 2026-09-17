use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use alcedo_common::context::ExtractContext;
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::plugins::InstallOverride;
use crate::db::plugin_migrations::{plugin_schema_name_for_scope, PluginMigrationEngine};
use crate::db::queries::PluginVersion;
use crate::db::schema::get_table_schemas;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;

pub async fn get_plugin_schema(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    let schema_name = plugin_schema_name_for_scope(&slug, plugin.app_version_id, plugin.version_id);

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
    let plugin_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
    let path = std::path::Path::new(&plugin_dir)
        .join(slug)
        .join(version)
        .join(subdir);

    if path.exists() {
        return Some(path);
    }
    let path = std::path::Path::new(&plugin_dir).join(slug).join(subdir);
    if path.exists() {
        return Some(path);
    }
    if subdir == "migrations" {
        let old_path = std::path::Path::new(&plugin_dir)
            .join("plugin-migrations")
            .join(slug);
        if old_path.exists() {
            return Some(old_path);
        }
    }

    None
}

/// Filesystem directory for admin-uploaded migrations belonging to a single
/// install. Namespaced by install id so a plugin slug with several installs
/// (global/version/app) cannot have one install's uploads shadow a sibling's.
pub fn plugin_upload_migrations_dir(slug: &str, install_id: i64) -> std::path::PathBuf {
    let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
    std::path::Path::new(&plugins_dir)
        .join("plugin-migrations")
        .join(slug)
        .join(format!("install-{}", install_id))
}

/// Resolve the migrations directory to read for a specific install.
///
/// Priority:
/// 1. `{PLUGINS_DIR}/{slug}/{version}/migrations` (migrations extracted from the image)
/// 2. `{PLUGINS_DIR}/{slug}/migrations` (local/extracted layout)
/// 3. `{PLUGINS_DIR}/plugin-migrations/{slug}/install-{install_id}` (admin uploads,
///    install-scoped)
/// 4. `{PLUGINS_DIR}/plugin-migrations/{slug}` (legacy slug-shared upload dir,
///    read-only backward compatibility)
///
/// Falls back to the (possibly absent) install-scoped dir so callers can
/// distinguish "no migrations" via `exists()`.
pub fn resolve_migrations_dir(
    slug: &str,
    install_id: i64,
    version: Option<&str>,
) -> std::path::PathBuf {
    let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
    let base = std::path::Path::new(&plugins_dir);

    if let Some(ver) = version {
        let path = base.join(slug).join(ver).join("migrations");
        if path.exists() {
            return path;
        }
    }
    let path = base.join(slug).join("migrations");
    if path.exists() {
        return path;
    }
    let install_scoped = plugin_upload_migrations_dir(slug, install_id);
    if install_scoped.exists() {
        return install_scoped;
    }
    let legacy = base.join("plugin-migrations").join(slug);
    if dir_has_sql_file(&legacy) {
        return legacy;
    }
    install_scoped
}

/// True when `dir` exists and holds at least one top-level `.sql` file. Guards
/// the legacy slug-shared fallback so a container directory created as the
/// parent of install-scoped uploads is not mistaken for a migrations directory.
fn dir_has_sql_file(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".sql")
            })
        })
        .unwrap_or(false)
}

pub async fn list_migrations(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    let version = PluginVersion::find_active_for_install(db_pool, plugin.id)
        .await?
        .map(|v| v.version);
    let migrations_dir = resolve_migrations_dir(&slug, plugin.id, version.as_deref());

    if !migrations_dir.exists() {
        return Ok(Json(vec![]));
    }

    let engine = PluginMigrationEngine::new_with_schema(
        db_pool.clone(),
        migrations_dir,
        &slug,
        plugin_schema_name_for_scope(&slug, plugin.app_version_id, plugin.version_id),
    );

    let statuses = engine.get_migration_status().await?;

    let result: Vec<serde_json::Value> = statuses
        .into_iter()
        .map(|s| {
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
        })
        .collect();

    Ok(Json(result))
}

pub async fn run_migration(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let version = PluginVersion::find_active_for_install(db_pool, plugin.id)
        .await?
        .map(|v| v.version);
    let migrations_dir = resolve_migrations_dir(&slug, plugin.id, version.as_deref());

    if !migrations_dir.exists() {
        return Err(AppError::NotFound(format!(
            "No migrations directory found for plugin {}",
            slug
        )));
    }

    let engine = PluginMigrationEngine::new_with_schema(
        db_pool.clone(),
        migrations_dir,
        &slug,
        plugin_schema_name_for_scope(&slug, plugin.app_version_id, plugin.version_id),
    );

    match engine.run_migrations().await {
        Ok(ran) => {
            PluginVersion::update_status(
                db_pool,
                plugin.id,
                &version.unwrap_or_default(),
                "running",
            )
            .await?;
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
        Err(e) => Err(AppError::Internal(format!(
            "Failed to run migrations: {}",
            e
        ))),
    }
}

pub async fn rollback_migration(
    headers: HeaderMap,
    Path((slug, target_version)): Path<(String, String)>,
    State(state): State<Arc<PluginAppState>>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("Database not configured".to_string()))?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let version = PluginVersion::find_active_for_install(db_pool, plugin.id)
        .await?
        .map(|v| v.version);
    let migrations_dir = resolve_migrations_dir(&slug, plugin.id, version.as_deref());

    if !migrations_dir.exists() {
        return Err(AppError::NotFound(format!(
            "No migrations directory found for plugin {}",
            slug
        )));
    }

    let engine = PluginMigrationEngine::new_with_schema(
        db_pool.clone(),
        migrations_dir,
        &slug,
        plugin_schema_name_for_scope(&slug, plugin.app_version_id, plugin.version_id),
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
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<crate::api::admin::UploadMigrationsRequest>,
) -> Result<Json<crate::api::admin::UploadMigrationsResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;
    let plugin = crate::api::install::resolve_install_for_request_authorized(
        &state,
        &headers,
        db_pool,
        &slug,
        &ctx,
        q.install_id,
    )
    .await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;

    let migrations_dir = plugin_upload_migrations_dir(&slug, plugin.id);

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
                error: Some(
                    "Invalid filename: contains path separator or directory reference".to_string(),
                ),
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

    Ok(Json(crate::api::admin::UploadMigrationsResponse {
        results,
    }))
}
