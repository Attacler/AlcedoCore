use serde_json::{Map, Value};

use std::collections::HashMap;

use sqlx::Row;

use crate::{
    AppState, item_map,
    migrations::app_migrations::m00011_seed_app_roles::seed_roles,
    services::{
        apps::AppsService,
        collections::schema::SchemaService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        versions::clone_production::{
            clone_schema_batched, copy_set_tables, recreate_schema_foreign_keys,
        },
    },
    utils::slugify,
};

pub const PRODUCTION_VERSION: &str = "production";

fn system_context(collection: &str) -> (AppContext, String) {
    (
        AppContext::system(RequestSource::API),
        collection.to_string(),
    )
}

pub struct VersionsService<'a> {
    pub(crate) app_state: &'a AppState,
}

impl VersionsService<'_> {
    pub fn new(state: &AppState) -> VersionsService<'_> {
        VersionsService { app_state: state }
    }

    pub async fn ensure_exists(&self, version_id: i32) -> Result<(), AlcedoError> {
        let (context, collection) = system_context("alcedocore_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["id".to_string()],
                limit: 0,
                ..Query::eq("id", Value::from(version_id))
            })
            .await?;
        if rows.is_empty() {
            return Err(AlcedoError::NotFound(
                format!("Version not found: {}", version_id),
                0,
            ));
        }
        Ok(())
    }

    pub async fn get_version_id_by_name(
        &self,
        version_name: &str,
    ) -> Result<Option<i32>, AlcedoError> {
        let (context, collection) = system_context("alcedocore_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows: Vec<Map<String, Value>> = service
            .read_items_by_query(Query {
                fields: vec!["id".to_string()],
                limit: 0,
                ..Query::eq("version_name", Value::String(version_name.to_string()))
            })
            .await?;
        Ok(rows
            .first()
            .and_then(|row| row.get("id"))
            .and_then(Value::as_i64)
            .map(|v| v as i32))
    }

    pub async fn get(&self, version_id: i32) -> Result<Option<Map<String, Value>>, AlcedoError> {
        let (context, collection) = system_context("alcedocore_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("id", Value::from(version_id))
            })
            .await?;
        Ok(rows.into_iter().next())
    }

    pub async fn delete(&self, version_id: i32) -> Result<bool, AlcedoError> {
        let (context, collection) = system_context("alcedocore_versions");
        let mut service = ItemsService::new(self.app_state, &context, &collection);
        let deleted = service
            .delete_items_by_pks(vec![Value::String(version_id.to_string())], None)
            .await?;
        Ok(deleted > 0)
    }

    pub async fn list(&self) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        let (context, collection) = system_context("alcedocore_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);

        let mut rows = service
            .read_items_by_query(Query {
                fields: vec!["id".to_string(), "version_name".to_string()],
                sort: vec!["+id".to_string()],
                limit: 0,
                ..Default::default()
            })
            .await?;

        // Attach the apps already deployed from each version (recorded in the
        // global `version_deployed` markers) so callers can hide deployed
        // `app_created` edits.
        let markers = sqlx::query(
            "SELECT metadata FROM alcedocore.alcedocore_system_logs \
             WHERE action = 'version_deployed'",
        )
        .fetch_all(&*self.app_state.database_pool)
        .await?;
        let mut deployed: HashMap<String, Vec<String>> = HashMap::new();
        for row in markers {
            let Some(meta) = row.try_get::<Option<Value>, _>("metadata").ok().flatten() else {
                continue;
            };
            let Some(vname) = meta.get("version_name").and_then(Value::as_str) else {
                continue;
            };
            let Some(applied) = meta.get("applied").and_then(Value::as_object) else {
                continue;
            };
            let entry = deployed.entry(vname.to_string()).or_default();
            for (_app, e) in applied {
                if let Some(apps) = e.get("apps").and_then(Value::as_array) {
                    for a in apps {
                        if let Some(name) = a.as_str() {
                            let name = name.to_string();
                            if !entry.contains(&name) {
                                entry.push(name);
                            }
                        }
                    }
                }
            }
        }
        for row in &mut rows {
            let name = row
                .get("version_name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let apps: Vec<Value> = deployed
                .get(name)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(Value::String)
                .collect();
            row.insert("deployed_apps".to_string(), Value::Array(apps));
        }
        Ok(rows)
    }

    pub async fn create(&self, version_name: &str) -> Result<Map<String, Value>, AlcedoError> {
        if self.get_version_id_by_name(version_name).await?.is_some() {
            return Err(AlcedoError::InvalidInput(
                format!("Version '{}' already exists", version_name),
                0,
            ));
        }

        let (context, collection) = system_context("alcedocore_versions");
        let mut service = ItemsService::new(self.app_state, &context, &collection);

        // The existence check above runs outside a transaction, so two racing
        // creates of the same name could both pass it. Serialize them with a
        // transaction-scoped advisory lock keyed on the name and re-check
        // inside the tx; the unique index from m00019 is the backstop if a
        // race still slips through.
        let mut tx = self.app_state.database_pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('version:' || $1))")
            .bind(version_name)
            .execute(&mut *tx)
            .await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM alcedocore.alcedocore_versions WHERE version_name = $1)",
        )
        .bind(version_name)
        .fetch_one(&mut *tx)
        .await?;
        if exists {
            return Err(AlcedoError::InvalidInput(
                format!("Version '{}' already exists", version_name),
                0,
            ));
        }

        let map = item_map! { "version_name" => version_name };
        let created = match service.create_many(vec![map], &mut Some(&mut tx)).await {
            Ok(created) => created,
            Err(e) => {
                // Backstop: the m00019 unique index fires if a concurrent
                // insert of the same name slipped past the advisory lock.
                if let AlcedoError::Other(msg, _) = &e {
                    if msg.contains("alcedocore_versions_version_name_key") {
                        return Err(AlcedoError::InvalidInput(
                            format!("Version '{}' already exists", version_name),
                            0,
                        ));
                    }
                }
                return Err(e);
            }
        };
        tx.commit().await?;
        service.run_after_commit().await;
        let version_id = created
            .first()
            .and_then(|pk| pk.parse::<i32>().ok())
            .ok_or_else(|| {
                AlcedoError::SystemError("Version insert returned no id".to_string(), 0)
            })?;

        let rows = service
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("id", Value::from(version_id))
            })
            .await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| AlcedoError::SystemError("Version was not created".to_string(), 0))
    }

    pub async fn ensure_default(&self) -> Result<(), AlcedoError> {
        if self
            .get_version_id_by_name(PRODUCTION_VERSION)
            .await?
            .is_some()
        {
            return Ok(());
        }

        let (context, collection) = system_context("alcedocore_versions");
        let mut service = ItemsService::new(self.app_state, &context, &collection);

        let map = item_map! { "version_name" => PRODUCTION_VERSION };
        service.create_many(vec![map], &mut None).await?;
        Ok(())
    }

    pub async fn app_api_names_for_version(
        &self,
        version_id: i32,
    ) -> Result<Vec<String>, AlcedoError> {
        let (context, collection) = system_context("alcedocore_apps_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["app_id.*".to_string()],
                limit: 0,
                ..Query::eq("version_id", Value::from(version_id))
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|row| {
                row.get("app_id")?
                    .get("api_name")?
                    .as_str()
                    .map(String::from)
            })
            .collect())
    }

    pub async fn delete_version_links(&self, version_id: i32) -> Result<(), AlcedoError> {
        let (context, collection) = system_context("alcedocore_apps_versions");
        let mut service = ItemsService::new(self.app_state, &context, &collection);
        service
            .delete_items_by_query(Query::eq("version_id", Value::from(version_id)), &mut None)
            .await?;
        Ok(())
    }

    pub async fn clone_production_apps(
        &self,
        target_version_id: i32,
        target_name: &str,
    ) -> Result<(), AlcedoError> {
        self.clone_apps_to_version(target_version_id, target_name, None, true)
            .await
    }

    /// Clone production's app schemas into the version `target_name`.
    ///
    /// `only_app_ids` restricts the clone to the given apps (deploy backups
    /// only need the apps the deploy is about to overwrite). `full` controls
    /// the expensive extras for a real, user-facing version: schema-cache
    /// refreshes, role seeding and FK recreation. Deploy backups pass
    /// `full = false` — they are only read back via raw SQL when restoring,
    /// and the deploy refreshes the cache once at its end.
    pub(crate) async fn clone_apps_to_version(
        &self,
        target_version_id: i32,
        target_name: &str,
        only_app_ids: Option<&[i32]>,
        full: bool,
    ) -> Result<(), AlcedoError> {
        let Some(production_id) = self.get_version_id_by_name(PRODUCTION_VERSION).await? else {
            return Ok(());
        };
        if production_id == target_version_id {
            return Ok(());
        }

        let pool = &*self.app_state.database_pool;
        let links = sqlx::query(
            "SELECT av.app_id, a.api_name \
             FROM alcedocore.alcedocore_apps_versions av \
             JOIN alcedocore.alcedocore_apps a ON a.id = av.app_id \
             WHERE av.version_id = $1 ORDER BY av.app_id",
        )
        .bind(production_id)
        .fetch_all(pool)
        .await?;

        // Decode the link rows up front and propagate decode errors instead of
        // silently dropping apps from the clone set.
        let mut apps: Vec<(i32, String)> = Vec::with_capacity(links.len());
        for row in links {
            let app_id = row.try_get::<i32, _>("app_id").map_err(|e| {
                AlcedoError::SystemError(
                    format!("Failed to decode app_id from alcedocore_apps_versions: {}", e),
                    0,
                )
            })?;
            if let Some(ids) = only_app_ids {
                if !ids.contains(&app_id) {
                    continue;
                }
            }
            let api_name = row.try_get::<String, _>("api_name").map_err(|e| {
                AlcedoError::SystemError(
                    format!("Failed to decode api_name from alcedocore_apps_versions: {}", e),
                    0,
                )
            })?;
            apps.push((app_id, api_name));
        }
        if apps.is_empty() {
            return Ok(());
        }

        let target_name = slugify(target_name);

        // Clone each app's schema BEFORE linking the app to the target
        // version: a failed clone must not leave the app linked to a version
        // whose schema does not exist.
        let mut cloned: Vec<(i32, String, String, String)> = Vec::new(); // (app_id, api_name, source, target)
        let mut failures: Vec<(i32, String)> = Vec::new(); // (app_id, error)
        for (app_id, api_name) in &apps {
            let source_schema = format!("{}010production", api_name);
            let target_schema = format!("{}010{}", api_name, target_name);
            // Copy set: all alcedocore_* system tables except permissions and
            // logs, plus the migrator state. User tables stay structure-only.
            let copy_tables = match copy_set_tables(pool, &source_schema).await {
                Ok(tables) => tables,
                Err(e) => {
                    failures.push((
                        *app_id,
                        format!("failed to list copy set of {}: {}", source_schema, e),
                    ));
                    continue;
                }
            };
            match clone_schema_batched(
                (*self.app_state.database_pool).clone(),
                source_schema.clone(),
                target_schema.clone(),
                copy_tables,
            )
            .await
            {
                Ok(()) => cloned.push((*app_id, api_name.clone(), source_schema, target_schema)),
                Err(e) => failures.push((
                    *app_id,
                    format!(
                        "failed to clone schema {} to {}: {}",
                        source_schema, target_schema, e
                    ),
                )),
            }
        }

        // Link only the apps whose clone succeeded.
        let pairs: Vec<(i32, i32)> = cloned
            .iter()
            .map(|(app_id, ..)| (*app_id, target_version_id))
            .collect();
        if !pairs.is_empty() {
            AppsService::new(self.app_state)
                .link_apps_to_version(&pairs)
                .await?;
        }

        if !failures.is_empty() {
            // Remove any link for an app whose clone failed (clone-before-link
            // means this run never linked it, but an earlier partial run may
            // have) — remove the link, never drop schemas here.
            for (app_id, _) in &failures {
                sqlx::query(
                    "DELETE FROM alcedocore.alcedocore_apps_versions \
                     WHERE version_id = $1 AND app_id = $2",
                )
                .bind(target_version_id)
                .bind(app_id)
                .execute(pool)
                .await?;
            }
            return Err(AlcedoError::SystemError(
                format!(
                    "Failed to clone {} app(s) into version '{}': {}",
                    failures.len(),
                    target_name,
                    failures
                        .iter()
                        .map(|(_, msg)| msg.clone())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                0,
            ));
        }

        if !full {
            return Ok(());
        }

        SchemaService::refresh_schema_and_meta(self.app_state).await?;

        // The clone copies `_sqlx_migrator_migrations`, so m00011 does not
        // re-run for the new schemas, and the copy set no longer carries
        // roles/policies — re-run just the (idempotent) role seed so the new
        // version gets the default admin/public roles and app-admin grants.
        // Seeding happens BEFORE the FKs are recreated: FKs that reference
        // alcedocore_roles are created NOT VALID, but the seeded roles must
        // exist before anything validates against them.
        for (_app_id, api_name, _source_schema, _target_schema) in &cloned {
            let app_context = AppContext {
                app_name: api_name.clone(),
                version: target_name.clone(),
                request_source: RequestSource::Migration,
                identity: None,
                request_id: None,
            };
            seed_roles(self.app_state, &app_context).await?;
        }
        // Recreate the FKs only after the roles are seeded.
        for (_app_id, _api_name, source_schema, target_schema) in &cloned {
            recreate_schema_foreign_keys(&self.app_state.database_pool, source_schema, target_schema)
                .await
                .map_err(|e| {
                    AlcedoError::SystemError(
                        format!(
                            "Failed to recreate foreign keys of {} from {}: {}",
                            target_schema, source_schema, e
                        ),
                        0,
                    )
                })?;
        }
        SchemaService::refresh_schema_and_meta(self.app_state).await?;
        Ok(())
    }
}
