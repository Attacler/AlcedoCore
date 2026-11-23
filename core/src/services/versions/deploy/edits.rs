//! Collecting a version's **edits** from its per-app schemas' activity logs
//! (rows written since the version row was created), plus the synthetic
//! `app_created` edits for apps linked to the version but not to production.

use std::collections::HashMap;

use serde_json::{Value, json};
use sqlx::{PgPool, Row};

use super::{
    AppLink, DeployEdit, applied::applied_edits_full_for_version, qtable, schema_exists,
};
use crate::{
    services::{
        errors::AlcedoError,
        versions::clone_production::copy_set_tables,
    },
};

pub(crate) fn edit_json(e: &DeployEdit, reason: &str) -> Value {
    json!({
        "id": e.id,
        "app": e.app,
        "source": e.source,
        "action": e.action,
        "target": e.target,
        "collection": e.collection,
        "item_id": e.item_id,
        "reason": reason,
    })
}

/// Whether an edit can be applied row-by-row in selected mode: `app_created`
/// edits trigger the new-app flow; system-log edits only when their target is
/// in the app's copy set. Collection-log (user item) edits are not deployable.
pub(crate) fn is_applicable(e: &DeployEdit, copy_tables: Option<&Vec<String>>) -> bool {
    match e.source.as_str() {
        "app" => true,
        "system" => e
            .target
            .as_ref()
            .map(|t| {
                copy_tables
                    .map(|ts| ts.iter().any(|x| x == t))
                    .unwrap_or(false)
            })
            .unwrap_or(false),
        _ => false,
    }
}

/// The edits of a version plus each linked app's copy set.
pub(crate) struct CollectedEdits {
    pub edits: Vec<DeployEdit>,
    pub copy_tables_by_app: HashMap<String, Vec<String>>,
}

/// Enumerates the version's pending edits: system-log and collection-log rows
/// of every linked app's version schema since the version was created (minus
/// the ones already applied by a previous deploy), plus `app_created` edits
/// for apps not linked to production.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn collect_edits(
    pool: &PgPool,
    version_id: i32,
    version_name: &str,
    version_slug: &str,
    version_links: &[AppLink],
    prod_by_app: &HashMap<i32, AppLink>,
    selected_ids: &Option<Vec<i64>>,
    selected_apps: &Option<Vec<String>>,
) -> Result<CollectedEdits, AlcedoError> {
    // Copy set of each linked app's version schema.
    let mut copy_tables_by_app: HashMap<String, Vec<String>> = HashMap::new();
    for link in version_links {
        let ver_schema = format!("{}010{}", link.api_name, version_slug);
        if schema_exists(pool, &ver_schema).await? {
            let tables = copy_set_tables(pool, &ver_schema).await.map_err(|e| {
                AlcedoError::SystemError(
                    format!("Failed to list copy set of {}: {}", ver_schema, e),
                    0,
                )
            })?;
            copy_tables_by_app.insert(link.api_name.clone(), tables);
        }
    }

    // Edits already applied by a previous deploy of this version (recorded in
    // the global `version_deployed` markers) are not pending anymore. Use the
    // full marker set so collection-log ids are filtered too — wholesale
    // deploys record them as handled (item rows themselves are never copied,
    // but the edit is no longer pending).
    let applied = applied_edits_full_for_version(pool, version_name).await?;
    let mut edits: Vec<DeployEdit> = Vec::new();
    for link in version_links {
        let schema = format!("{}010{}", link.api_name, version_slug);
        if !schema_exists(pool, &schema).await? {
            continue;
        }
        let sys_rows = sqlx::query(&format!(
            "SELECT id, action, target, metadata, metadata->>'id' AS meta_id, \
                    metadata->>'name' AS meta_name, created_at::text AS created_at \
             FROM {} \
             WHERE created_at >= (SELECT created_at FROM alcedocore.alcedocore_versions WHERE id = $1) \
               AND ($2::bigint[] IS NULL OR id = ANY($2)) \
             ORDER BY id",
            qtable(&schema, "alcedocore_system_logs")
        ))
        .bind(version_id)
        .bind(selected_ids)
        .fetch_all(pool)
        .await?;
        for row in &sys_rows {
            let id: Option<i64> = row.try_get("id").ok();
            if let Some(id) = id {
                if applied
                    .get(&link.api_name)
                    .map(|e| e.system_ids.contains(&id))
                    .unwrap_or(false)
                {
                    continue;
                }
            }
            edits.push(DeployEdit {
                id,
                app: link.api_name.clone(),
                source: "system".to_string(),
                action: row.get::<String, _>("action"),
                target: row.try_get("target").ok(),
                collection: None,
                item_id: row
                    .try_get::<Option<String>, _>("meta_id")
                    .ok()
                    .flatten()
                    .map(|id| json!(id)),
                created_at: row.try_get("created_at").ok(),
                meta_name: row.try_get("meta_name").ok(),
                meta: row.try_get("metadata").ok(),
            });
        }
        let col_rows = sqlx::query(&format!(
            "SELECT id, action, collection_name, item_id, created_at::text AS created_at \
             FROM {} \
             WHERE created_at >= (SELECT created_at FROM alcedocore.alcedocore_versions WHERE id = $1) \
               AND ($2::bigint[] IS NULL OR id = ANY($2)) \
             ORDER BY id",
            qtable(&schema, "alcedocore_collection_logs")
        ))
        .bind(version_id)
        .bind(selected_ids)
        .fetch_all(pool)
        .await?;
        for row in &col_rows {
            let id: Option<i64> = row.try_get("id").ok();
            if let Some(id) = id {
                if applied
                    .get(&link.api_name)
                    .map(|e| e.collection_ids.contains(&id))
                    .unwrap_or(false)
                {
                    continue;
                }
            }
            edits.push(DeployEdit {
                id: row.try_get("id").ok(),
                app: link.api_name.clone(),
                source: "collection".to_string(),
                action: row.get::<String, _>("action"),
                target: None,
                collection: row.try_get("collection_name").ok(),
                item_id: row.try_get("item_id").ok(),
                created_at: row.try_get("created_at").ok(),
                meta_name: None,
                meta: None,
            });
        }
    }
    // New apps (linked to the version but not to production) are edits too.
    for link in version_links {
        if !prod_by_app.contains_key(&link.app_id) {
            let already_deployed = applied
                .get(&link.api_name)
                .map(|e| e.apps.contains(&link.api_name))
                .unwrap_or(false);
            let selected = match selected_apps {
                None => true,
                Some(apps) => apps.iter().any(|a| a == &link.api_name),
            };
            if selected && !already_deployed {
                edits.push(DeployEdit {
                    id: None,
                    app: link.api_name.clone(),
                    source: "app".to_string(),
                    action: "app_created".to_string(),
                    target: Some(link.api_name.clone()),
                    collection: None,
                    item_id: None,
                    created_at: None,
                    meta_name: None,
                    meta: None,
                });
            }
        }
    }

    Ok(CollectedEdits {
        edits,
        copy_tables_by_app,
    })
}
