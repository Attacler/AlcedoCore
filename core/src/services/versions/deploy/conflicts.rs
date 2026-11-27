//! The conflict check: for every pending edit, look whether the same object
//! was also changed in production after the version's clone timestamp. Runs
//! in THREE queries per app (production system logs, production collection
//! logs, production relation names since the clone) and matches in memory —
//! no per-edit queries.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use sqlx::PgPool;

use super::{AppLink, DeployConflict, DeployEdit, edit_key, is_applicable, qtable, schema_relations};
use crate::services::errors::AlcedoError;

fn both_sides_conflict(
    app: &str,
    edit: &DeployEdit,
    idx: usize,
    collection: Option<String>,
    prod_action: String,
    detail: String,
) -> DeployConflict {
    DeployConflict {
        conflict_type: "changed_on_both_sides".to_string(),
        app: app.to_string(),
        collection,
        item_id: edit.item_id.clone(),
        detail,
        prod_action: Some(prod_action),
        version_action: Some(edit.action.clone()),
        edit_idx: idx,
    }
}

fn missing_relation_conflict(
    app: &str,
    edit: &DeployEdit,
    idx: usize,
    collection: String,
    prod_schema: &str,
) -> DeployConflict {
    DeployConflict {
        conflict_type: "relation_missing_in_prod".to_string(),
        app: app.to_string(),
        collection: Some(collection.clone()),
        item_id: edit.item_id.clone(),
        detail: format!("collection '{}' does not exist in {}", collection, prod_schema),
        prod_action: None,
        version_action: Some(edit.action.clone()),
        edit_idx: idx,
    }
}

/// Checks every edit of every app that also exists in production against
/// production's logs after the version's clone timestamp. New apps have
/// nothing to conflict with and are skipped.
pub(crate) async fn check_conflicts(
    pool: &PgPool,
    version_id: i32,
    version_name: &str,
    version_links: &[AppLink],
    prod_by_app: &HashMap<i32, AppLink>,
    edits: &[DeployEdit],
) -> Result<Vec<DeployConflict>, AlcedoError> {
    let mut conflicts: Vec<DeployConflict> = Vec::new();
    for link in version_links {
        if !prod_by_app.contains_key(&link.app_id) {
            continue; // new app: nothing to conflict with in production
        }
        let prod_schema = format!("{}010production", link.api_name);

        // ONE query: production system-log actions since the version was cloned.
        let sys_rows: Vec<(String, String, Option<String>)> = sqlx::query_as(&format!(
            "SELECT action, target, metadata->>'id' AS meta_id FROM {} \
             WHERE created_at > (SELECT created_at FROM alcedocore.alcedocore_versions WHERE id = $1)",
            qtable(&prod_schema, "alcedocore_system_logs")
        ))
        .bind(version_id)
        .fetch_all(pool)
        .await?;
        let mut sys_by_target: HashMap<String, Vec<(Option<String>, String)>> = HashMap::new();
        for (action, target, meta_id) in sys_rows {
            sys_by_target.entry(target).or_default().push((meta_id, action));
        }

        // ONE query: production collection-log actions since the clone.
        let col_rows: Vec<(String, String, Value)> = sqlx::query_as(&format!(
            "SELECT action, collection_name, item_id FROM {} \
             WHERE created_at > (SELECT created_at FROM alcedocore.alcedocore_versions WHERE id = $1)",
            qtable(&prod_schema, "alcedocore_collection_logs")
        ))
        .bind(version_id)
        .fetch_all(pool)
        .await?;
        let mut col_by_name: HashMap<String, Vec<(Value, String)>> = HashMap::new();
        for (action, collection, item_id) in col_rows {
            col_by_name
                .entry(collection)
                .or_default()
                .push((item_id, action));
        }

        // ONE query: the relations that exist in the production schema.
        let relations: HashSet<String> = schema_relations(pool, &prod_schema).await?;

        for (idx, edit) in edits
            .iter()
            .enumerate()
            .filter(|(_, e)| e.app == link.api_name)
        {
            match edit.source.as_str() {
                "system" => {
                    let target = edit.target.clone().unwrap_or_default();
                    let meta_id: Option<String> = edit
                        .item_id
                        .as_ref()
                        .and_then(|v| v.as_str().map(String::from));
                    // Same semantics as the old per-edit query
                    // `($3::text IS NULL OR metadata->>'id' = $3)`: a NULL
                    // meta_id matches any production change of the target.
                    if let Some(prod_action) = sys_by_target.get(&target).and_then(|rows| {
                        rows.iter()
                            .find(|(m, _)| meta_id.is_none() || *m == meta_id)
                            .map(|(_, action)| action.clone())
                    }) {
                        conflicts.push(both_sides_conflict(
                            &link.api_name,
                            edit,
                            idx,
                            None,
                            prod_action,
                            format!(
                                "'{}' was changed in both production and version '{}'",
                                target, version_name
                            ),
                        ));
                    }
                    // A collection created in the version must exist in production.
                    if edit.action == "collection_created" {
                        if let Some(name) = &edit.meta_name {
                            if !relations.contains(name) {
                                conflicts.push(missing_relation_conflict(
                                    &link.api_name,
                                    edit,
                                    idx,
                                    name.clone(),
                                    &prod_schema,
                                ));
                            }
                        }
                    }
                }
                "collection" => {
                    let collection = edit.collection.clone().unwrap_or_default();
                    let item_id = edit.item_id.clone().unwrap_or(Value::Null);
                    if let Some(prod_action) = col_by_name.get(&collection).and_then(|rows| {
                        rows.iter()
                            .find(|(iid, _)| *iid == item_id)
                            .map(|(_, action)| action.clone())
                    }) {
                        conflicts.push(both_sides_conflict(
                            &link.api_name,
                            edit,
                            idx,
                            Some(collection.clone()),
                            prod_action,
                            format!(
                                "item {} of '{}' was changed in both production and version '{}'",
                                item_id, collection, version_name
                            ),
                        ));
                    }
                    if !relations.contains(&collection) {
                        conflicts.push(missing_relation_conflict(
                            &link.api_name,
                            edit,
                            idx,
                            collection,
                            &prod_schema,
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(conflicts)
}

/// The physical signature of a field row: the `options` keys that map to
/// physical column/constraint properties. A change to any of them cannot be
/// applied to an existing production table by a deploy (columns are only ever
/// added, never altered or dropped; FKs only added, never retargeted; unique /
/// required constraints are not recreated on existing columns).
/// (type, relationship_type, related_collection, related_app, unique, required)
fn field_signature(options: &Value) -> (String, String, String, String, String, String) {
    let get = |k: &str| {
        options
            .get(k)
            .and_then(|v| {
                v.as_bool()
                    .map(|b| b.to_string())
                    .or_else(|| v.as_str().map(String::from))
            })
            .unwrap_or_default()
    };
    (
        get("type"),
        get("relationship_type"),
        get("related_collection"),
        get("related_app"),
        get("unique"),
        get("required"),
    )
}

/// Human-readable reason for a signature difference.
fn sig_detail(
    field: &str,
    coll: &str,
    prod_sig: &(String, String, String, String, String, String),
    ver_sig: &(String, String, String, String, String, String),
    prod_schema: &str,
) -> String {
    if prod_sig.0 != ver_sig.0 {
        format!(
            "field '{field}' of collection '{coll}' changed type '{}' → '{}' in the version; a deploy cannot alter the physical column in {prod_schema}",
            prod_sig.0, ver_sig.0
        )
    } else if prod_sig.1 != ver_sig.1 || prod_sig.2 != ver_sig.2 || prod_sig.3 != ver_sig.3 {
        format!(
            "field '{field}' of collection '{coll}' changed its relation target in the version; a deploy cannot retarget the physical FK in {prod_schema}"
        )
    } else {
        format!(
            "field '{field}' of collection '{coll}' changed its unique/required constraint in the version; a deploy cannot alter the physical constraint in {prod_schema}"
        )
    }
}

/// Field rows of one schema, keyed by `"{collection_id}:{api_name}"` (the
/// stable identity of a field — ids can be reused after delete+recreate) and
/// additionally by `"id:{id}"` for edits whose metadata lacks the pair.
/// Returns key → (api_name, collection table, options).
async fn field_rows(
    pool: &PgPool,
    schema: &str,
    collection_ids: &[i64],
    ids: &[String],
) -> Result<HashMap<String, (String, Option<String>, Value)>, AlcedoError> {
    let rows: Vec<(String, String, String, Option<String>, Value)> = sqlx::query_as(&format!(
        "SELECT f.collection_id::text || ':' || f.api_name AS key, f.id::text, f.api_name, \
                c.\"table\", f.options \
         FROM {} f \
         LEFT JOIN {} c ON c.id = f.collection_id \
         WHERE f.collection_id = ANY($1) OR f.id::text = ANY($2)",
        qtable(schema, "alcedocore_fields"),
        qtable(schema, "alcedocore_collections"),
    ))
    .bind(collection_ids.to_vec())
    .bind(ids.to_vec())
    .fetch_all(pool)
    .await?;
    let mut map = HashMap::new();
    for (key, id, name, coll, opts) in rows {
        map.insert(key, (name.clone(), coll.clone(), opts.clone()));
        map.insert(format!("id:{}", id), (name, coll, opts));
    }
    Ok(map)
}

/// All field rows of one schema, keyed by `"{collection_id}:{api_name}"` only
/// (the wholesale comparison has no edit metadata to fall back to).
async fn all_field_rows(
    pool: &PgPool,
    schema: &str,
) -> Result<HashMap<String, (String, Option<String>, Value)>, AlcedoError> {
    let rows: Vec<(String, String, Option<String>, Option<Value>)> = sqlx::query_as(&format!(
        "SELECT f.collection_id::text || ':' || f.api_name AS key, f.api_name, \
                c.\"table\", f.options \
         FROM {} f \
         LEFT JOIN {} c ON c.id = f.collection_id",
        qtable(schema, "alcedocore_fields"),
        qtable(schema, "alcedocore_collections"),
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(key, name, coll, opts)| (key, (name, coll, opts.unwrap_or(Value::Null))))
        .collect())
}

/// Pre-write validation: field-level changes that a deploy cannot apply
/// physically to an EXISTING production table are refused up front, before
/// anything is written:
///
/// - `field_deleted` — the version removed the field; a deploy would delete
///   the metadata row in production but leave the physical column behind.
/// - `field_type_change` — the version changed the field's type or relation
///   target; a deploy would copy the metadata row but leave the physical
///   column type / FK in production unchanged.
///
/// New fields (no row in production yet) are fine — the column/FK sync adds
/// them. Apps not linked to production are skipped (everything is new there).
/// Runs in TWO queries per app with field edits.
///
/// Wholesale deploys TRUNCATE + refill the whole copy set, so a field row
/// changed WITHOUT a log row (e.g. a type change via the collections PUT,
/// which writes raw SQL) would reach production as metadata only. When the
/// app is actually written wholesale, EVERY version field row is compared
/// against production, not just the logged edits.
pub(crate) async fn check_unsupported_field_changes(
    pool: &PgPool,
    version_slug: &str,
    version_links: &[AppLink],
    prod_by_app: &HashMap<i32, AppLink>,
    edits: &[DeployEdit],
    skipped: &HashSet<usize>,
    copy_tables_by_app: &HashMap<String, Vec<String>>,
    selected_mode: bool,
) -> Result<Vec<DeployConflict>, AlcedoError> {
    let mut unsupported: Vec<DeployConflict> = Vec::new();
    for link in version_links {
        if !prod_by_app.contains_key(&link.app_id) {
            continue; // new app: nothing existing to refuse
        }
        let ver_schema = format!("{}010{}", link.api_name, version_slug);
        let prod_schema = format!("{}010production", link.api_name);

        // Wholesale mode: the copy set is TRUNCATE + refilled and the repair
        // path syncs missing user-table columns even for apps WITHOUT pending
        // edits, so EVERY field row of every production-linked app is compared
        // against production — not just logged edits.
        let wholesale = !selected_mode;
        if wholesale {
            let trigger_idx = edits
                .iter()
                .position(|e| {
                    e.app == link.api_name
                        && is_applicable(e, copy_tables_by_app.get(&link.api_name))
                })
                .unwrap_or(0);
            let ver_rows = all_field_rows(pool, &ver_schema).await?;
            let prod_rows = all_field_rows(pool, &prod_schema).await?;
            for (key, (ver_name, ver_coll, ver_opts)) in &ver_rows {
                let Some((_, prod_coll, prod_opts)) = prod_rows.get(key) else {
                    continue; // new field in the version: the column/FK sync adds it
                };
                let coll = ver_coll
                    .clone()
                    .or_else(|| prod_coll.clone())
                    .unwrap_or_default();
                let prod_sig = field_signature(prod_opts);
                let ver_sig = field_signature(ver_opts);
                if prod_sig == ver_sig {
                    continue;
                }
                let detail = sig_detail(ver_name, &coll, &prod_sig, &ver_sig, &prod_schema);
                unsupported.push(DeployConflict {
                    conflict_type: "field_type_change".to_string(),
                    app: link.api_name.clone(),
                    collection: Some(coll),
                    item_id: Some(json!(key)),
                    detail,
                    prod_action: None,
                    version_action: None,
                    edit_idx: trigger_idx,
                });
            }
            // Wholesale: the all-rows pass above already flagged every
            // type/relation change; the per-edit pass below still catches
            // logged field DELETIONS.
        }

        // The field edits of this app (applicable, not skipped by on_conflict),
        // keyed by the field's stable identity `{collection_id}:{api_name}`
        // (falling back to `id:{id}` when the metadata lacks the pair — ids can
        // be reused after a delete+recreate, so they are not reliable alone).
        let mut field_edits: Vec<(usize, String)> = Vec::new();
        let mut collection_ids: Vec<i64> = Vec::new();
        let mut fallback_ids: Vec<String> = Vec::new();
        for (idx, e) in edits.iter().enumerate() {
            if e.app != link.api_name
                || skipped.contains(&idx)
                || e.source != "system"
                || e.target.as_deref() != Some("alcedocore_fields")
                || !is_applicable(e, copy_tables_by_app.get(&link.api_name))
            {
                continue;
            }
            let meta = e.meta.as_ref();
            let cid = meta
                .and_then(|m| m.get("collection_id"))
                .and_then(Value::as_i64);
            let name = meta
                .and_then(|m| m.get("api_name"))
                .and_then(Value::as_str);
            match (cid, name) {
                (Some(c), Some(n)) => {
                    collection_ids.push(c);
                    field_edits.push((idx, format!("{}:{}", c, n)));
                }
                _ => {
                    if let Some(id) = edit_key(meta, e.item_id.as_ref(), "id") {
                        fallback_ids.push(id.clone());
                        field_edits.push((idx, format!("id:{}", id)));
                    }
                }
            }
        }
        if field_edits.is_empty() {
            continue;
        }
        let ver_rows = field_rows(pool, &ver_schema, &collection_ids, &fallback_ids).await?;
        let prod_rows = field_rows(pool, &prod_schema, &collection_ids, &fallback_ids).await?;

        for (idx, key) in field_edits {
            let Some((prod_name, prod_coll, prod_opts)) = prod_rows.get(&key) else {
                continue; // new field in the version: the column/FK sync adds it
            };
            let coll = prod_coll.clone().unwrap_or_default();
            let Some((ver_name, ver_coll, ver_opts)) = ver_rows.get(&key) else {
                // The field row is gone in the version: the field was deleted.
                unsupported.push(DeployConflict {
                    conflict_type: "field_deleted".to_string(),
                    app: link.api_name.clone(),
                    collection: Some(coll.clone()),
                    item_id: Some(json!(key)),
                    detail: format!(
                        "field '{}' of collection '{}' was deleted in the version; a deploy cannot drop the physical column in {}",
                        prod_name, coll, prod_schema
                    ),
                    prod_action: None,
                    version_action: Some(edits[idx].action.clone()),
                    edit_idx: idx,
                });
                continue;
            };
            if wholesale {
                continue; // sig diffs on rows present in both schemas are flagged by the all-rows pass
            }
            let ver_coll = ver_coll.clone().unwrap_or(coll.clone());
            let prod_sig = field_signature(prod_opts);
            let ver_sig = field_signature(ver_opts);
            if prod_sig != ver_sig {
                let detail = sig_detail(ver_name, &ver_coll, &prod_sig, &ver_sig, &prod_schema);
                unsupported.push(DeployConflict {
                    conflict_type: "field_type_change".to_string(),
                    app: link.api_name.clone(),
                    collection: Some(ver_coll),
                    item_id: Some(json!(key)),
                    detail,
                    prod_action: None,
                    version_action: Some(edits[idx].action.clone()),
                    edit_idx: idx,
                });
            }
        }
    }
    Ok(unsupported)
}
