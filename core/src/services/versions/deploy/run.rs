//! The `deploy` entry point: validate → collect edits → conflict check →
//! (unless dry_run / fail-on-conflict) write per app → copy plugin rows →
//! audit row → refresh the schema cache.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::{
    AppLink, DeployResponse, DeployVersionRequest, NO_BACKUP_WARNING, OnConflict,
    acquire_deploy_lock, app_links, check_conflicts, check_unsupported_field_changes,
    collect_edits, edit_json, is_applicable, schema_exists, write_app,
};
use crate::{
    services::{
        activity_logs,
        collections::schema::SchemaService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        route_mappings::RouteMappingsService,
        versions::{
            clone_production::{exec_multi, missing_user_columns_sql},
            service::{PRODUCTION_VERSION, VersionsService},
        },
    },
    utils::slugify,
};

/// What a deploy has written so far (per-app write results and the applied
/// edit map). Threaded through [`VersionsService::deploy_locked`] so that a
/// failure — per-app writes commit individually, so a deploy can fail
/// half-applied — can still record a partial `version_deployed` audit row.
#[derive(Default)]
struct DeployProgress {
    apps_result: Vec<Value>,
    applied: HashMap<String, Value>,
    deployed_links: Vec<AppLink>,
}

impl VersionsService<'_> {
    /// Deploy version `version_id` into `production`. See the module docs.
    pub async fn deploy(
        &self,
        version_id: i32,
        opts: &DeployVersionRequest,
    ) -> Result<DeployResponse, AlcedoError> {
        // Serialize deploys and reverts in this process AND across all core
        // replicas: the Postgres session-level advisory lock is held on a
        // dedicated pooled connection for the whole deploy.
        let pool = &*self.app_state.database_pool;
        let lock = acquire_deploy_lock(pool).await?;

        let mut progress = DeployProgress::default();
        match self.deploy_locked(version_id, opts, &mut progress).await {
            Ok(resp) => {
                lock.release().await;
                Ok(resp)
            }
            Err(err) => {
                // A failed deploy may have partially applied (per-app
                // transactions commit individually): record a failed
                // `version_deployed` audit row — carrying whatever was
                // applied — before returning the error, so partial deploys
                // are visible in the system log. Then release the lock.
                let _ = self
                    .record_failed_deploy(version_id, opts, &progress, &err)
                    .await;
                lock.release().await;
                Err(err)
            }
        }
    }

    /// Writes a `version_deployed` audit row with `"status": "failed"` for a
    /// deploy that returned an error, so partial deploys are visible (and
    /// their applied edits remain revertable via the `applied` map).
    async fn record_failed_deploy(
        &self,
        version_id: i32,
        opts: &DeployVersionRequest,
        progress: &DeployProgress,
        err: &AlcedoError,
    ) -> Result<(), AlcedoError> {
        let pool = &*self.app_state.database_pool;
        let version_name: String = sqlx::query_as(
            "SELECT version_name FROM alcedocore.alcedocore_versions WHERE id = $1",
        )
        .bind(version_id)
        .fetch_optional(pool)
        .await?
        .map(|(n,)| n)
        .unwrap_or_else(|| format!("version#{}", version_id));
        let ctx = AppContext::system(RequestSource::API);
        activity_logs::record(
            self.app_state,
            &ctx,
            "version_deployed",
            &version_name,
            Some(format!(
                "Deploy of version '{}' to production FAILED (no data backup taken): {}",
                version_name, err
            )),
            json!({
                "version_id": version_id,
                "version_name": version_name,
                "status": "failed",
                "error": err.to_string(),
                "dry_run": opts.dry_run,
                "on_conflict": opts.on_conflict,
                "apps": progress.apps_result,
                "applied": progress.applied,
                "warning": NO_BACKUP_WARNING,
            }),
            None,
            None,
            None,
            None,
        )
        .await
    }

    /// The deploy body, run under the deploy lock (see [`Self::deploy`]).
    async fn deploy_locked(
        &self,
        version_id: i32,
        opts: &DeployVersionRequest,
        progress: &mut DeployProgress,
    ) -> Result<DeployResponse, AlcedoError> {
        let pool = &*self.app_state.database_pool;

        let version: Option<(String,)> = sqlx::query_as(
            "SELECT version_name FROM alcedocore.alcedocore_versions WHERE id = $1",
        )
        .bind(version_id)
        .fetch_optional(pool)
        .await?;
        let Some((version_name,)) = version else {
            return Err(AlcedoError::NotFound(
                format!("Version not found: {}", version_id),
                0,
            ));
        };
        if version_name == PRODUCTION_VERSION {
            return Err(AlcedoError::InvalidInput(
                "The production version cannot be deployed onto itself".to_string(),
                0,
            ));
        }
        let production_id = self
            .get_version_id_by_name(PRODUCTION_VERSION)
            .await?
            .ok_or_else(|| {
                AlcedoError::SystemError("The production version is missing".to_string(), 0)
            })?;

        let version_links = app_links(pool, version_id).await?;
        if version_links.is_empty() {
            return Err(AlcedoError::InvalidInput(
                format!("Version '{}' has no linked apps", version_name),
                0,
            ));
        }
        let prod_links = app_links(pool, production_id).await?;
        let prod_by_app: HashMap<i32, AppLink> = prod_links
            .into_iter()
            .map(|l| (l.app_id, l))
            .collect();

        let version_slug = slugify(&version_name);

        // edit_ids: numeric entries select log rows, string entries (app
        // api_names) select app_created edits; anything else is ignored.
        let selected_ids: Option<Vec<i64>> = opts
            .edit_ids
            .as_ref()
            .map(|ids| ids.iter().filter_map(|v| v.as_i64()).collect());
        let selected_apps: Option<Vec<String>> = opts
            .edit_ids
            .as_ref()
            .map(|ids| ids.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        let selected_mode = opts.edit_ids.is_some();

        // ---- edits: the version's log rows since the version was created ----
        let collected = collect_edits(
            pool,
            version_id,
            &version_name,
            &version_slug,
            &version_links,
            &prod_by_app,
            &selected_ids,
            &selected_apps,
        )
        .await?;
        let edits = collected.edits;
        let copy_tables_by_app = collected.copy_tables_by_app;

        // ---- conflict check (also in dry_run); runs on the selected edits only ----
        let conflicts = check_conflicts(
            pool,
            version_id,
            &version_name,
            &version_links,
            &prod_by_app,
            &edits,
        )
        .await?;

        // Edits excluded by on_conflict = "skip".
        let skipped_indices: HashSet<usize> = if opts.on_conflict == OnConflict::Skip {
            conflicts
                .iter()
                .filter(|c| c.conflict_type == "changed_on_both_sides")
                .map(|c| c.edit_idx)
                .collect()
        } else {
            HashSet::new()
        };
        let mut skipped: Vec<Value> = skipped_indices
            .iter()
            .map(|&i| edit_json(&edits[i], "conflict with production"))
            .collect();
        if selected_mode {
            for (i, e) in edits.iter().enumerate() {
                if skipped_indices.contains(&i) {
                    continue;
                }
                let reason = match e.source.as_str() {
                    "collection" => Some("user items are excluded from the copy set"),
                    "system" => {
                        if is_applicable(e, copy_tables_by_app.get(&e.app)) {
                            None
                        } else {
                            Some("target is not in the copy set")
                        }
                    }
                    _ => None,
                };
                if let Some(r) = reason {
                    skipped.push(edit_json(e, r));
                }
            }
        }

        // ---- unsupported field changes (pre-write, also in dry_run) ----
        // Field type changes and field deletions on existing collections
        // cannot be applied physically by a deploy; refuse before writing
        // anything (regardless of on_conflict — the physical change is
        // impossible, not just conflicting).
        let unsupported = check_unsupported_field_changes(
            pool,
            &version_slug,
            &version_links,
            &prod_by_app,
            &edits,
            &skipped_indices,
            &copy_tables_by_app,
        )
        .await?;

        let base = json!({
            "version_id": version_id,
            "version_name": version_name,
            "dry_run": opts.dry_run,
            "edits": edits,
            "conflicts": conflicts,
            "unsupported": unsupported,
            "skipped": skipped,
            "warning": NO_BACKUP_WARNING,
        });

        let conflict_fail = !conflicts.is_empty() && opts.on_conflict == OnConflict::Fail;
        if !opts.dry_run && (conflict_fail || !unsupported.is_empty()) {
            let mut data = base.clone();
            data["deployed"] = json!(false);
            data["on_conflict"] = json!(opts.on_conflict);
            if !unsupported.is_empty() {
                data["message"] = json!(
                    "The version contains field changes a deploy cannot apply (field type change or field deletion on an existing collection). Nothing was written."
                );
            }
            return Ok(DeployResponse {
                ok: false,
                data,
            });
        }

        if opts.dry_run {
            let mut data = base.clone();
            data["deployed"] = json!(false);
            return Ok(DeployResponse { ok: true, data });
        }

        // ---- write path ----

        // Which apps will actually be written?
        let mut deploy_apps: Vec<&AppLink> = Vec::new();
        for link in &version_links {
            let write = match prod_by_app.get(&link.app_id) {
                Some(_) => {
                    // Wholesale mode also only writes apps that have at least
                    // one pending applicable edit: an app the version never
                    // touched has nothing to deploy, and overwriting it would
                    // clobber production-side changes made after the clone.
                    edits.iter().enumerate().any(|(i, e)| {
                        e.app == link.api_name
                            && !skipped_indices.contains(&i)
                            && is_applicable(e, copy_tables_by_app.get(&link.api_name))
                    })
                }
                None => {
                    !selected_mode
                        || edits.iter().any(|e| {
                            e.app == link.api_name && e.source == "app" && e.action == "app_created"
                        })
                }
            };
            if write {
                deploy_apps.push(link);
            }
        }

        // Order the apps to write so cross-app relations deploy dependencies
        // first: an app whose version schema has FKs into another app's
        // version schema (`related_app` fields) must be written after that
        // app, because its recreated FKs point at `{other}010production`,
        // which must already exist.
        let ordered_apps = order_apps_by_cross_app_deps(pool, &version_slug, &deploy_apps).await?;

        // 1. Per-app apply: version schema -> production schema. May be empty
        //    (e.g. only plugin installs changed): plugin rows are still copied
        //    below, so don't early-return here.
        for link in &ordered_apps {
            if !deploy_apps.iter().any(|l| l.app_id == link.app_id) {
                continue;
            }
            let w = write_app(
                self,
                link,
                &version_slug,
                production_id,
                &prod_by_app,
                selected_mode,
                &edits,
                &skipped_indices,
                &copy_tables_by_app,
            )
            .await?;
            progress.applied.insert(
                w.app.clone(),
                json!({
                    "system_ids": w.system_ids,
                    "collection_ids": w.collection_ids,
                    "apps": w.apps,
                }),
            );
            progress.apps_result.push(w.result);
            progress.deployed_links.push((*link).clone());
        }

        // Wholesale mode: apps without pending edits are not overwritten (that
        // would clobber production-side changes), but their user-table columns
        // are still synced — additive only (ADD COLUMN IF NOT EXISTS). Deploys
        // made before the column sync existed copied field metadata without
        // the physical column, so a re-deploy must be able to complete that.
        let mut synced_schemas: Vec<String> = Vec::new();
        if !selected_mode {
            for link in &version_links {
                if deploy_apps.iter().any(|l| l.app_id == link.app_id) {
                    continue; // already written above (sync ran inside write_app)
                }
                if !prod_by_app.contains_key(&link.app_id) {
                    continue; // new app: handled by the write loop
                }
                let ver_schema = format!("{}010{}", link.api_name, version_slug);
                if !schema_exists(pool, &ver_schema).await? {
                    continue;
                }
                let prod_schema = format!("{}010production", link.api_name);
                let mut conn = pool.acquire().await?;
                let sync = missing_user_columns_sql(&mut *conn, &ver_schema, &prod_schema, None)
                    .await
                    .map_err(|e| {
                        AlcedoError::SystemError(
                            format!("Failed to sync user-table columns of {}: {}", ver_schema, e),
                            0,
                        )
                    })?;
                if !sync.is_empty() {
                    exec_multi(&mut *conn, &sync).await.map_err(|e| {
                        AlcedoError::SystemError(
                            format!("Failed to sync user-table columns of {}: {}", ver_schema, e),
                            0,
                        )
                    })?;
                    synced_schemas.push(prod_schema);
                }
            }
        }

        // 2. Plugins + route mappings. Recompute production's app-version links
        //    AFTER linking new apps, so installs of new apps are remapped to
        //    their production app-version ids (installs remapped to production's
        //    app-version ids, route mappings remapped to the new install ids).
        //    Wholesale mode copies the installs of ALL linked apps (a version
        //    can carry plugin changes without any schema edits); selected mode
        //    only the apps that were actually written.
        let prod_by_app: HashMap<i32, AppLink> = app_links(pool, production_id)
            .await?
            .into_iter()
            .map(|l| (l.app_id, l))
            .collect();
        let plugin_links: Vec<AppLink> = if selected_mode {
            progress.deployed_links.clone()
        } else {
            version_links.clone()
        };
        let plugin_results = self
            .copy_plugin_rows(&plugin_links, &prod_by_app)
            .await?;
        RouteMappingsService::new(self.app_state).sync_cache().await?;

        // 3. Refresh the schema cache, then write the audit row in the global
        //    system log. The `applied` map records which edits this deploy
        //    applied, per app, so later deploys and the log endpoints can hide
        //    them as no longer pending. (Refresh runs first so that nothing
        //    fallible happens after the audit row is written.)
        let mut prod_schemas: Vec<String> = progress
            .deployed_links
            .iter()
            .map(|l| format!("{}010production", l.api_name))
            .collect();
        prod_schemas.extend(synced_schemas);
        SchemaService::refresh_schemas_scoped(self.app_state, &prod_schemas).await?;

        let ctx = AppContext::system(RequestSource::API);
        activity_logs::record(
            self.app_state,
            &ctx,
            "version_deployed",
            &version_name,
            Some(format!(
                "Deployed version '{}' to production (no data backup taken)",
                version_name
            )),
            json!({
                "version_id": version_id,
                "version_name": version_name,
                "apps": progress.apps_result,
                "applied": progress.applied,
                "on_conflict": opts.on_conflict,
                "conflicts": conflicts,
                "skipped": skipped,
            }),
            None,
            None,
            None,
            None,
        )
        .await?;

        let mut data = base.clone();
        data["deployed"] = json!(true);
        data["apps"] = json!(progress.apps_result);
        data["applied"] = json!(progress.applied);
        data["plugins"] = json!(plugin_results);
        data["on_conflict"] = json!(opts.on_conflict);
        Ok(DeployResponse { ok: true, data })
    }
}

/// Extracts the referenced schema from an FK definition
/// (`REFERENCES "schema"."table"` or `REFERENCES schema.table`).
fn fk_referenced_schema(fk_def: &str) -> Option<String> {
    let rest = fk_def.split("REFERENCES ").nth(1)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let end = rest.find(['"', '.'])?;
    Some(rest[..end].to_string())
}

/// Topologically orders the apps to deploy so that an app referenced by
/// another app's cross-app FKs (`related_app` relation fields, which point at
/// `{other_app}010{version_slug}` in the version schema) is written first.
/// Stable: apps without dependencies keep their original relative order;
/// dependency cycles fall back to the original order.
async fn order_apps_by_cross_app_deps<'a>(
    pool: &sqlx::PgPool,
    version_slug: &str,
    deploy_apps: &[&'a AppLink],
) -> Result<Vec<&'a AppLink>, AlcedoError> {
    use std::collections::{HashMap as Map, HashSet as Set};
    // app api_name -> set of apps it depends on (its version schema has FKs
    // into their version schemas)
    let mut depends_on: Map<String, Set<String>> = Map::new();
    let names: Set<&str> = deploy_apps.iter().map(|l| l.api_name.as_str()).collect();
    for link in deploy_apps {
        let ver_schema = format!("{}010{}", link.api_name, version_slug);
        if !schema_exists(pool, &ver_schema).await? {
            continue;
        }
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT pg_get_constraintdef(c.oid) FROM pg_constraint c \
             JOIN pg_class cl ON cl.oid = c.conrelid \
             JOIN pg_namespace n ON n.oid = cl.relnamespace \
             WHERE c.contype = 'f' AND n.nspname = $1",
        )
        .bind(&ver_schema)
        .fetch_all(pool)
        .await?;
        for (def,) in rows {
            let Some(ref_schema) = fk_referenced_schema(&def) else { continue };
            let Some((other_app, ver)) = ref_schema.split_once("010") else { continue };
            if ver == version_slug && other_app != link.api_name && names.contains(other_app) {
                depends_on
                    .entry(link.api_name.clone())
                    .or_default()
                    .insert(other_app.to_string());
            }
        }
    }
    // Kahn's algorithm, stable by original order.
    let mut remaining: Vec<&AppLink> = deploy_apps.to_vec();
    let mut ordered: Vec<&AppLink> = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let mut progressed = false;
        let mut i = 0;
        while i < remaining.len() {
            let name = &remaining[i].api_name;
            let blocked = depends_on
                .get(name.as_str())
                .map(|ds| ds.iter().any(|d| remaining.iter().any(|l| &l.api_name == d)))
                .unwrap_or(false);
            if !blocked {
                ordered.push(remaining.remove(i));
                progressed = true;
            } else {
                i += 1;
            }
        }
        if !progressed {
            // cycle: keep the rest in original order
            ordered.extend(remaining.drain(..));
        }
    }
    Ok(ordered)
}
