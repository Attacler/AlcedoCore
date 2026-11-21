//! Copying plugin catalog rows, install rows and route mappings from the
//! version to production (install ids remapped to production's app-version
//! ids, route mappings remapped to the new install ids).

use std::collections::HashMap;

use serde_json::{Value, json};
use sqlx::{Acquire, Row};

use super::{AppLink, table_columns};
use crate::{
    services::{
        errors::AlcedoError,
        event_forwarder::{invalidate_manifest_cache, invalidate_subscribers},
        plugin_db_access::{apply_scoped_database_url, ensure_install_db_access},
        plugin_migrations::PluginMigrationEngine,
        plugins::{
            DeployInput, IMAGE_DOCS, IMAGE_MANIFEST, IMAGE_MIGRATIONS, IMAGE_PAGES, IMAGE_PUBLIC,
            IMAGE_ROOTS, PluginsService, image_candidate, invalidate_install_cache, plugin_dir,
            plugin_env, plugin_subdir, read_migration_files,
        },
        versions::{clone_production::qident, service::VersionsService},
    },
    utils::{image_reference, normalize_image_reference},
};

impl VersionsService<'_> {
    /// Copies the plugin catalog rows used by the version's installs and the
    /// install rows themselves, remapping `app_version_id` to production's
    /// app-version id (new install ids), then the route mappings that point at
    /// those installs, remapping `install_id` to the new install ids.
    /// Re-deploying the same version does not duplicate installs: an existing
    /// install on (plugin_id, production app_version_id) is reused.
    pub(crate) async fn copy_plugin_rows(
        &self,
        version_links: &[AppLink],
        prod_by_app: &HashMap<i32, AppLink>,
    ) -> Result<Vec<Value>, AlcedoError> {
        let pool = &*self.app_state.database_pool;
        let av_ids: Vec<i32> = version_links.iter().map(|l| l.app_version_id).collect();
        if av_ids.is_empty() {
            return Ok(Vec::new());
        }
        let installs = sqlx::query(
            "SELECT * FROM alcedocore.alcedocore_plugins_installs WHERE app_version_id = ANY($1) ORDER BY id",
        )
        .bind(&av_ids)
        .fetch_all(pool)
        .await?;

        let install_cols = table_columns(pool, "alcedocore", "alcedocore_plugins_installs").await?;
        let install_copy: Vec<String> = install_cols
            .iter()
            .filter(|c| *c != "id" && *c != "app_version_id")
            .cloned()
            .collect();
        let plugin_cols = table_columns(pool, "alcedocore", "alcedocore_plugins").await?;
        let plugin_copy: Vec<String> =
            plugin_cols.iter().filter(|c| *c != "id").cloned().collect();

        let mut results: Vec<Value> = Vec::new();
        let mut install_map: HashMap<i32, i32> = HashMap::new(); // old install id -> new
        // All plugin-ROW changes (catalog upsert, install insert/update,
        // route-mapping delete/update) run in ONE transaction on a dedicated
        // connection, so a failure rolls back all of them instead of leaving
        // a half-copied plugin state. Runtime provisioning (files, migrations,
        // platform deploy) is not a row change and runs AFTER the commit.
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;
        // (old install id, new install id, plugin id, prod app_version id)
        let mut to_provision: Vec<(i32, i32, i32, i32)> = Vec::new();
        // (new install id, prod app_version id)
        let mut to_invalidate: Vec<(i32, i32)> = Vec::new();
        for row in &installs {
            let old_id: i32 = row.get("id");
            let av_id: i32 = row.get("app_version_id");
            let Some(link) = version_links.iter().find(|l| l.app_version_id == av_id) else {
                continue;
            };
            let Some(prod_link) = prod_by_app.get(&link.app_id) else {
                continue;
            };
            let plugin_id: i32 = row.get("plugin_id");

            // Catalog row (upsert by slug).
            let cols = plugin_copy
                .iter()
                .map(|c| qident(c))
                .collect::<Vec<_>>()
                .join(", ");
            let set_clause = plugin_copy
                .iter()
                .filter(|c| *c != "slug")
                .map(|c| format!("{} = EXCLUDED.{}", qident(c), qident(c)))
                .collect::<Vec<_>>()
                .join(", ");
            sqlx::query(&format!(
                "INSERT INTO alcedocore.alcedocore_plugins ({cols}) \
                 SELECT {cols} FROM alcedocore.alcedocore_plugins WHERE id = $1 \
                 ON CONFLICT (slug) DO UPDATE SET {set_clause}",
            ))
            .bind(plugin_id)
            .execute(&mut *tx)
            .await?;

            // Install row with remapped app_version_id — deduplicated on
            // (plugin_id, app_version_id) so re-deploys don't duplicate.
            let existing: Option<(i32, String)> = sqlx::query_as(
                "SELECT id, plugin_version FROM alcedocore.alcedocore_plugins_installs \
                 WHERE plugin_id = $1 AND app_version_id = $2",
            )
            .bind(plugin_id)
            .bind(prod_link.app_version_id)
            .fetch_optional(&mut *tx)
            .await?;
            let plugin_version: String = row.get("plugin_version");
            let (new_id, is_new, version_changed) = match existing {
                Some((id, prod_version)) => {
                    // Re-deploy: the version wins on the install's mutable
                    // fields; a changed plugin_version re-provisions the runtime.
                    sqlx::query(
                        "UPDATE alcedocore.alcedocore_plugins_installs \
                         SET plugin_version = $3, settings = $4, granted_scopes = $5, enabled = $6, \
                             updated_at = now() \
                         WHERE id = $1",
                    )
                    .bind(id)
                    .bind(plugin_id)
                    .bind(&plugin_version)
                    .bind(row.try_get::<Option<Value>, _>("settings").ok().flatten())
                    .bind(row.try_get::<Option<Value>, _>("granted_scopes").ok().flatten())
                    .bind(row.try_get::<Option<bool>, _>("enabled").ok().flatten())
                    .execute(&mut *tx)
                    .await?;
                    (id, false, prod_version != plugin_version)
                }
                None => {
                    let cols = install_copy
                        .iter()
                        .map(|c| qident(c))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let id: i32 = sqlx::query_scalar(&format!(
                        "INSERT INTO alcedocore.alcedocore_plugins_installs ({cols}, app_version_id) \
                         SELECT {cols}, $2 FROM alcedocore.alcedocore_plugins_installs WHERE id = $1 \
                         RETURNING id",
                    ))
                    .bind(old_id)
                    .bind(prod_link.app_version_id)
                    .fetch_one(&mut *tx)
                    .await?;
                    (id, true, true)
                }
            };
            // A new install (or a new plugin version) needs the actual runtime:
            // DB role/schema, extracted files, migrations and the platform
            // deployment (Swarm service on the docker platform) — copying the
            // rows alone leaves the plugin installed but not running. The
            // provisioning itself runs after the transaction commits (it must
            // not hold the row transaction open, and its own DB writes would
            // deadlock against the uncommitted install row).
            if is_new || version_changed {
                to_provision.push((old_id, new_id, plugin_id, prod_link.app_version_id));
            } else {
                // Mutable fields (settings/scopes/enabled) changed on an
                // existing install: drop the cached ResolvedInstall and the
                // subscriber list so the next proxy/event read sees them.
                to_invalidate.push((new_id, prod_link.app_version_id));
            }
            install_map.insert(old_id, new_id);
            results.push(json!({
                "install_id": old_id,
                "new_install_id": new_id,
                "plugin_id": plugin_id,
                "app": link.api_name,
                "runtime_provisioned": is_new || version_changed,
            }));
        }

        // Route mappings pointing at the version's installs. The table is
        // global (shared by all versions), so "copying with install_id
        // remapped" means remapping the existing row in place.
        let old_ids: Vec<i32> = install_map.keys().copied().collect();
        if !old_ids.is_empty() {
            let mappings = sqlx::query(
                "SELECT * FROM alcedocore.alcedocore_route_mappings WHERE install_id = ANY($1) ORDER BY id",
            )
            .bind(&old_ids)
            .fetch_all(&mut *tx)
            .await?;
            for m in &mappings {
                let old_rm_id: i32 = m.get("id");
                let old_install: i32 = m.get("install_id");
                let Some(&new_install) = install_map.get(&old_install) else {
                    continue;
                };
                let domain: Option<String> = m.get("domain");
                let path_prefix: Option<String> = m.get("path_prefix");
                // ON CONFLICT (domain, path_prefix) never matches a NULL domain
                // (NULLs are distinct in unique indexes), so remove a
                // conflicting mapping with an explicit delete before remapping.
                sqlx::query(
                    "DELETE FROM alcedocore.alcedocore_route_mappings \
                     WHERE domain IS NOT DISTINCT FROM $1 AND path_prefix IS NOT DISTINCT FROM $2 \
                       AND id <> $3",
                )
                .bind(&domain)
                .bind(&path_prefix)
                .bind(old_rm_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "UPDATE alcedocore.alcedocore_route_mappings SET install_id = $2 WHERE id = $1",
                )
                .bind(old_rm_id)
                .bind(new_install)
                .execute(&mut *tx)
                .await?;
            }
        }

        // One transaction for all plugin-row changes: commit here (any error
        // above rolled the whole copy back) before runtime provisioning.
        tx.commit().await?;

        // Runtime provisioning / cache invalidation, after the row
        // transaction committed (these do not participate in it).
        for (old_id, new_id, plugin_id, prod_app_version_id) in to_provision {
            let row = sqlx::query(
                "SELECT * FROM alcedocore.alcedocore_plugins_installs WHERE id = $1",
            )
            .bind(old_id)
            .fetch_one(pool)
            .await?;
            self.provision_install_runtime(new_id as i64, &row, plugin_id, prod_app_version_id)
                .await?;
        }
        for (new_id, prod_app_version_id) in to_invalidate {
            invalidate_install_cache(self.app_state, new_id as i64).await;
            invalidate_subscribers(self.app_state, prod_app_version_id).await;
        }

        Ok(results)
    }

    /// Provisions the runtime for a production install created/updated by a
    /// deploy: per-install DB role + schema, plugin files extracted from the
    /// image, plugin migrations, event-forwarder caches, and the platform
    /// deployment itself (Swarm service on the docker platform, directory on
    /// mock). Mirrors the runtime half of `PluginsService::deploy`.
    async fn provision_install_runtime(
        &self,
        install_id: i64,
        version_install: &sqlx::postgres::PgRow,
        plugin_id: i32,
        prod_app_version_id: i32,
    ) -> Result<(), AlcedoError> {
        let state = self.app_state;
        let pool = &*state.database_pool;
        let plugin_version: String = version_install.get("plugin_version");

        let catalog = sqlx::query(
            "SELECT slug, image FROM alcedocore.alcedocore_plugins WHERE id = $1",
        )
        .bind(plugin_id)
        .fetch_one(pool)
        .await?;
        let slug: String = catalog.get("slug");
        let image_raw: Option<String> = catalog.try_get("image").ok().flatten();
        let Some(image_raw) = image_raw.filter(|s| !s.trim().is_empty()) else {
            return Err(AlcedoError::InvalidInput(
                format!("Plugin '{}' has no image; cannot provision its runtime", slug),
                0,
            ));
        };
        let image = normalize_image_reference(&image_raw);

        // Per-install DB role + schema (best-effort, same as a normal deploy).
        if let Err(e) = ensure_install_db_access(pool, install_id).await {
            tracing::warn!(
                "[DEPLOY] Could not provision DB access for install {}: {}",
                install_id,
                e
            );
        }

        // Extract plugin files from the image into {PLUGINS_DIR}/{install_id}/.
        for (sub, sub_name) in [
            (IMAGE_DOCS, "docs"),
            (IMAGE_PAGES, "pages"),
            (IMAGE_MIGRATIONS, "migrations"),
            (IMAGE_PUBLIC, "public"),
            (IMAGE_MANIFEST, ""),
        ] {
            let dest = if sub_name.is_empty() {
                plugin_dir(&state.config, install_id)
            } else {
                plugin_subdir(&state.config, install_id, sub_name)
            };
            for root in IMAGE_ROOTS {
                let src = image_candidate(root, sub);
                match state
                    .platform
                    .extract_from_image(
                        &image_reference(&image, &plugin_version),
                        &plugin_version,
                        &src,
                        &dest,
                    )
                    .await
                {
                    Ok(true) => break,
                    Ok(false) => continue,
                    Err(e) => {
                        tracing::warn!(
                            "[DEPLOY] Could not extract {} from {}: {}",
                            src,
                            image,
                            e
                        );
                        break;
                    }
                }
            }
        }

        // Plugin migrations from the extracted migrations dir.
        let migrations_dir = plugin_subdir(&state.config, install_id, "migrations");
        if migrations_dir.exists() && !read_migration_files(&migrations_dir).is_empty() {
            let engine = PluginMigrationEngine::new(state, install_id, migrations_dir);
            let result = engine.run().await?;
            if !result.errors.is_empty() {
                return Err(AlcedoError::SystemError(
                    format!(
                        "Plugin migrations failed for install {}: {}",
                        install_id,
                        result.errors.join("; ")
                    ),
                    0,
                ));
            }
        }

        // Event-forwarder caches: the install's manifest plus the subscribers
        // of the production app-version it is bound to.
        invalidate_manifest_cache(state, install_id).await;
        invalidate_subscribers(state, prod_app_version_id).await;

        // The platform deployment (Swarm service on docker, dir on mock).
        let input = DeployInput {
            slug: slug.clone(),
            plugin_version: plugin_version.clone(),
            ..Default::default()
        };
        let mut env = plugin_env(&state.config, &input, install_id);
        apply_scoped_database_url(state, &mut env, install_id).await;
        let deployment_id = state
            .platform
            .deploy(&slug, &plugin_version, &image, env, Some(install_id))
            .await?;
        sqlx::query(
            "UPDATE alcedocore.alcedocore_plugins_installs \
             SET deployment_id = $2, updated_at = now() WHERE id = $1",
        )
        .bind(install_id)
        .bind(&deployment_id)
        .execute(pool)
        .await?;

        invalidate_install_cache(state, install_id).await;
        if let Err(e) = PluginsService::new(state)
            .record_version(install_id, &plugin_version, &deployment_id)
            .await
        {
            tracing::warn!(
                "[DEPLOY] Could not record version history for install {}: {}",
                install_id,
                e
            );
        }
        tracing::info!(
            "[DEPLOY] Provisioned runtime for plugin '{}' v{} (install {}, deployment {})",
            slug,
            plugin_version,
            install_id,
            deployment_id
        );
        Ok(())
    }
}
