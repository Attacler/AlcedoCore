//! The `version_deployed` / `version_reverted` audit markers and the app-link
//! lookup shared by deploy, revert and the log endpoints.

use std::collections::HashMap;

use serde_json::Value;
use sqlx::{PgPool, Row};

use crate::services::errors::AlcedoError;

#[derive(Clone)]
pub(crate) struct AppLink {
    pub app_version_id: i32,
    pub app_id: i32,
    pub api_name: String,
}

pub(crate) async fn app_links(pool: &PgPool, version_id: i32) -> Result<Vec<AppLink>, AlcedoError> {
    let rows = sqlx::query(
        "SELECT av.id AS app_version_id, av.app_id, a.api_name \
         FROM alcedocore.alcedocore_apps_versions av \
         JOIN alcedocore.alcedocore_apps a ON a.id = av.app_id \
         WHERE av.version_id = $1 ORDER BY av.id",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| AppLink {
            app_version_id: row.get("app_version_id"),
            app_id: row.get("app_id"),
            api_name: row.get("api_name"),
        })
        .collect())
}

/// Applied-edit markers of a version. Every successful deploy writes a
/// `version_deployed` row into the global system log whose metadata carries
/// `applied: { "<app>": { "system_ids": [...], "apps": [...] } }`. Returns
/// per-app (already-deployed system log ids, deployed new-app api_names) so
/// callers can hide edits that are no longer pending.
pub(crate) async fn applied_edits_for_version(
    pool: &PgPool,
    version_name: &str,
) -> Result<HashMap<String, (Vec<i64>, Vec<String>)>, AlcedoError> {
    let full = applied_edits_full_for_version(pool, version_name).await?;
    Ok(full
        .into_iter()
        .map(|(app, e)| (app, (e.system_ids, e.apps)))
        .collect())
}

/// Per-app record of what deploys of `version_name` applied (and what later
/// reverts undid): system-log ids, collection-log ids and new-app api_names.
#[derive(Debug, Default, Clone)]
pub(crate) struct AppliedEdits {
    pub system_ids: Vec<i64>,
    pub collection_ids: Vec<i64>,
    pub apps: Vec<String>,
}

/// Reads the `version_deployed` / `version_reverted` audit rows of a version.
/// Deployed ids are added, reverted ids subtracted, so the result is what is
/// currently applied.
pub(crate) async fn applied_edits_full_for_version(
    pool: &PgPool,
    version_name: &str,
) -> Result<HashMap<String, AppliedEdits>, AlcedoError> {
    let rows = sqlx::query(
        "SELECT action, metadata FROM alcedocore.alcedocore_system_logs \
         WHERE action IN ('version_deployed', 'version_reverted') AND metadata->>'version_name' = $1",
    )
    .bind(version_name)
    .fetch_all(pool)
    .await?;
    let mut map: HashMap<String, AppliedEdits> = HashMap::new();
    for row in rows {
        let reverted = row.get::<String, _>("action") == "version_reverted";
        let Some(applied) = row
            .try_get::<Option<Value>, _>("metadata")
            .ok()
            .flatten()
            .and_then(|m| m.get("applied").cloned())
        else {
            continue;
        };
        let Some(obj) = applied.as_object() else {
            continue;
        };
        for (app, entry) in obj {
            let sys = entry
                .get("system_ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect::<Vec<i64>>())
                .unwrap_or_default();
            let col = entry
                .get("collection_ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect::<Vec<i64>>())
                .unwrap_or_default();
            let apps = entry
                .get("apps")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default();
            let e = map.entry(app.clone()).or_default();
            if reverted {
                e.system_ids.retain(|id| !sys.contains(id));
                e.collection_ids.retain(|id| !col.contains(id));
                e.apps.retain(|a| !apps.contains(a));
            } else {
                e.system_ids.extend(sys);
                e.collection_ids.extend(col);
                e.apps.extend(apps);
            }
        }
    }
    Ok(map)
}
