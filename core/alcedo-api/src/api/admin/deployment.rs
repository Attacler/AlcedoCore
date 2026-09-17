use crate::db::queries::{PluginVersion, Registry};
use crate::error::AppError;
use crate::plugins::health::AppState;

pub async fn prepare_plugin_deployment(
    state: &AppState,
    db_pool: &sqlx::PgPool,
    registry: &Registry,
    slug: &str,
    version: &str,
    image: &str,
    install_id: i64,
    app_version_id: Option<i32>,
    version_id: Option<i32>,
) -> Result<(), AppError> {
    if PluginVersion::find_by_install_and_version(db_pool, install_id, version)
        .await?
        .is_none()
    {
        PluginVersion::insert(
            db_pool,
            &PluginVersion {
                id: 0,
                install_id,
                slug: slug.to_string(),
                version: version.to_string(),
                deployment_id: None,
                status: "deploying".to_string(),
                is_active: false,
                deployed_at: Some(chrono::Utc::now()),
                public_synced: false,
                public_path: None,
                pages_synced: false,
                pages_path: None,
            },
        )
        .await?;
    }

    let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());

    let mount_base = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
    let has_explicit_pvc = mount_base != "/var/lib/plugin-public";

    if has_explicit_pvc {
        let extract_base = std::path::Path::new(&mount_base)
            .join(slug)
            .join(version);

        if let Some(ref platform) = state.platform {
            let _ = platform
                .extract_from_image(
                    registry,
                    image,
                    "/app/migrations",
                    &extract_base.to_string_lossy(),
                )
                .await;
        }
        let migrations_dir = extract_base.join("migrations");
        let nested = migrations_dir.join("migrations");
        if nested.exists() && nested.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&nested) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let target = migrations_dir.join(&file_name);
                    let _ = std::fs::rename(entry.path(), &target);
                }
            }
            let _ = std::fs::remove_dir_all(&nested);
        }
        if migrations_dir.exists() {
            let has_migrations = std::fs::read_dir(&migrations_dir)
                .map(|mut d| d.any(|e| e.is_ok()))
                .unwrap_or(false);
            if has_migrations {
                tracing::info!(
                    "Running migrations for plugin {} from {:?}",
                    slug,
                    migrations_dir
                );
                crate::db::run_plugin_migrations_for_install(
                    db_pool,
                    slug,
                    &migrations_dir.to_string_lossy(),
                    app_version_id,
                    version_id,
                )
                .await?;
                let schema_name = crate::db::plugin_migrations::plugin_schema_name_for_scope(
                    slug,
                    app_version_id,
                    version_id,
                );
                let table_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = $1",
                )
                .bind(&schema_name)
                .fetch_one(db_pool)
                .await
                .unwrap_or(0);
                if table_count > 0 {
                    tracing::info!(
                        "Plugin {} schema '{}' has {} tables after migration",
                        slug,
                        schema_name,
                        table_count
                    );
                }
            } else {
                let _ = std::fs::remove_dir_all(&migrations_dir);
            }
        }

        if let Some(ref platform) = state.platform {
            for (src, extra_path) in &[
                ("/app/docs", ""),
                ("/app/pages/dist", "pages"),
                ("/app/public", ""),
            ] {
                let dest = extract_base.clone().join(extra_path);

                let _ = platform
                    .extract_from_image(registry, image, src, &dest.to_string_lossy())
                    .await;
            }
        }
    } else {
        let plugins_path = std::path::Path::new(&plugins_dir);

        let pages_dir = plugins_path.join(slug).join("pages");
        if pages_dir.exists() {
            let _ = std::fs::remove_dir_all(&pages_dir);
        }
        if let Some(ref platform) = state.platform {
            let _ = platform
                .extract_from_image(
                    registry,
                    image,
                    "/app/pages/dist",
                    &pages_dir.to_string_lossy(),
                )
                .await;
        }

        let slug_dir = plugins_path.join(slug);
        if let Some(ref platform) = state.platform {
            let _ = platform
                .extract_from_image(registry, image, "/app/public", &slug_dir.to_string_lossy())
                .await;
        }

        let docs_dir = plugins_path.join(slug).join("docs");
        if docs_dir.exists() {
            let _ = std::fs::remove_dir_all(&docs_dir);
        }
        if let Some(ref platform) = state.platform {
            let _ = platform
                .extract_from_image(registry, image, "/app/docs", &docs_dir.to_string_lossy())
                .await;
        }

        if let Ok(mount) = std::env::var("PLUGINS_DIR") {
            let pvc_base = std::path::Path::new(&mount).join(slug).join(version);
            if let Some(ref platform) = state.platform {
                for src in &["/app/docs", "/app/pages/dist", "/app/public"] {
                    let _ = platform
                        .extract_from_image(registry, image, src, &pvc_base.to_string_lossy())
                        .await;
                }
            }
        }
    }

    Ok(())
}
