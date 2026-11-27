//! `POST /api/platform/versions/{id}/revert`: undo what previous deploys of a
//! version applied to production, by applying the inverse of each recorded
//! edit (read back from the version's log rows) in reverse order. No data
//! backup exists — only the changes recorded in the logs are undone. Runs in
//! ONE transaction per app, so a failure changes nothing for that app.

use std::collections::HashMap;

use serde_json::{Value, json};
use sqlx::{Acquire, Row};

use super::{
    DeployResponse, NO_BACKUP_WARNING, acquire_deploy_lock, column_data_types, edit_key,
    pk_columns, qident, qtable, table_columns, table_exists,
    applied::applied_edits_full_for_version,
};
use crate::{
    services::{
        activity_logs,
        collections::schema::SchemaService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        versions::{
            clone_production::rewrite_collection_app_columns,
            service::{PRODUCTION_VERSION, VersionsService},
        },
    },
    utils::slugify,
};

impl VersionsService<'_> {
    /// Revert what previous deploys of `version_id` applied to production.
    pub async fn revert(&self, version_id: i32) -> Result<DeployResponse, AlcedoError> {
        let pool = &*self.app_state.database_pool;
        // Serialize reverts against deploys/reverts in this process AND
        // across all core replicas (Postgres session-level advisory lock,
        // released by the guard's drop when this returns).
        let _lock = acquire_deploy_lock(pool).await?;

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
                "The production version cannot be reverted".to_string(),
                0,
            ));
        }
        let version_slug = slugify(&version_name);
        let applied = applied_edits_full_for_version(pool, &version_name).await?;

        let mut reverted: Vec<Value> = Vec::new();
        let mut skipped: Vec<Value> = Vec::new();
        let mut reverted_out: HashMap<String, Value> = HashMap::new();
        let mut touched_schemas: Vec<String> = Vec::new();

        for (app, entry) in &applied {
            if entry.system_ids.is_empty() && entry.collection_ids.is_empty() && entry.apps.is_empty()
            {
                continue;
            }
            let prod_schema = format!("{}010production", app);
            let ver_schema = format!("{}010{}", app, version_slug);
            let mut conn = pool.acquire().await?;
            let mut tx = conn.begin().await?;

            // System-log (metadata/schema) edits, newest first.
            if !entry.system_ids.is_empty() {
                let rows = sqlx::query(&format!(
                    "SELECT id, action, target, metadata, metadata->>'name' AS meta_name \
                     FROM {} WHERE id = ANY($1) ORDER BY id DESC",
                    qtable(&ver_schema, "alcedocore_system_logs")
                ))
                .bind(&entry.system_ids)
                .fetch_all(&mut *tx)
                .await?;
                for row in rows {
                    let id: i64 = row.get("id");
                    let action: String = row.get("action");
                    let target: String = row.get("target");
                    let meta: Value = row
                        .try_get::<Option<Value>, _>("metadata")
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| json!({}));
                    let meta_name: Option<String> = row.try_get("meta_name").ok();
                    if !table_exists(&mut *tx, &prod_schema, &target).await? {
                        skipped.push(json!({ "id": id, "app": app, "source": "system", "action": action, "target": target, "reason": "target table missing in production" }));
                        continue;
                    }
                    let pk_cols = pk_columns(&mut *tx, &prod_schema, &target).await?;
                    let Some(pk) = (pk_cols.len() == 1).then(|| pk_cols[0].clone()) else {
                        skipped.push(json!({ "id": id, "app": app, "source": "system", "action": action, "target": target, "reason": "composite pk not supported" }));
                        continue;
                    };
                    let key = edit_key(Some(&meta), None, &pk);
                    if action.ends_with("_created") {
                        // Inverse of a create: delete the row (and its FK
                        // children first); for a created collection also drop
                        // its user table.
                        if let Some(key) = &key {
                            let fk_children: Vec<(String, String, String)> = sqlx::query_as(
                                "SELECT n.nspname, cl.relname, \
                                        (SELECT a.attname FROM pg_attribute a \
                                         WHERE a.attrelid = con.conrelid AND a.attnum = con.conkey[1]) \
                                 FROM pg_constraint con \
                                 JOIN pg_class cl ON cl.oid = con.conrelid \
                                 JOIN pg_namespace n ON n.oid = cl.relnamespace \
                                 WHERE con.confrelid = $1::regclass AND con.contype = 'f' \
                                   AND array_length(con.conkey, 1) = 1",
                            )
                            .bind(format!("{}.{}", qident(&prod_schema), qident(&target)))
                            .fetch_all(&mut *tx)
                            .await?;
                            for (child_schema, child_table, child_col) in &fk_children {
                                sqlx::query(&format!(
                                    "DELETE FROM {} WHERE {}::text = $1",
                                    qtable(child_schema, child_table),
                                    qident(child_col),
                                ))
                                .bind(key)
                                .execute(&mut *tx)
                                .await?;
                            }
                            sqlx::query(&format!(
                                "DELETE FROM {} WHERE {}::text = $1",
                                qtable(&prod_schema, &target),
                                qident(&pk),
                            ))
                            .bind(key)
                            .execute(&mut *tx)
                            .await?;
                        }
                        if action == "collection_created" {
                            if let Some(name) = &meta_name {
                                sqlx::query(&format!(
                                    "DROP TABLE IF EXISTS {}",
                                    qtable(&prod_schema, name)
                                ))
                                .execute(&mut *tx)
                                .await?;
                            }
                        }
                    } else if action.ends_with("_deleted") {
                        // Inverse of a delete: re-insert the old row from metadata.
                        sqlx::query(&format!(
                            "INSERT INTO {t} OVERRIDING SYSTEM VALUE \
                             SELECT * FROM jsonb_populate_record(NULL::{t}, $1::jsonb)",
                            t = qtable(&prod_schema, &target),
                        ))
                        .bind(&meta)
                        .execute(&mut *tx)
                        .await?;
                    } else {
                        // Inverse of an update: restore the old row (upsert).
                        let cols = table_columns(&mut *tx, &prod_schema, &target).await?;
                        let set_clause = cols
                            .iter()
                            .filter(|c| *c != &pk)
                            .map(|c| format!("{} = EXCLUDED.{}", qident(c), qident(c)))
                            .collect::<Vec<_>>()
                            .join(", ");
                        sqlx::query(&format!(
                            "INSERT INTO {t} OVERRIDING SYSTEM VALUE \
                             SELECT * FROM jsonb_populate_record(NULL::{t}, $1::jsonb) \
                             ON CONFLICT ({pk}) DO UPDATE SET {set_clause}",
                            t = qtable(&prod_schema, &target),
                            pk = qident(&pk),
                            set_clause = set_clause,
                        ))
                        .bind(&meta)
                        .execute(&mut *tx)
                        .await?;
                    }
                    reverted.push(json!({ "id": id, "app": app, "source": "system", "action": action, "target": target }));
                }
            }

            // Collection-log (user item) edits, newest first.
            if !entry.collection_ids.is_empty() {
                let rows = sqlx::query(&format!(
                    "SELECT id, action, collection_name, item_id, diff \
                     FROM {} WHERE id = ANY($1) ORDER BY id DESC",
                    qtable(&ver_schema, "alcedocore_collection_logs")
                ))
                .bind(&entry.collection_ids)
                .fetch_all(&mut *tx)
                .await?;
                for row in rows {
                    let id: i64 = row.get("id");
                    let action: String = row.get("action");
                    let collection: String = row.get("collection_name");
                    let item_id: Value = row.try_get("item_id").ok().unwrap_or(Value::Null);
                    let diff: Value = row
                        .try_get::<Option<Value>, _>("diff")
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| json!({}));
                    if !table_exists(&mut *tx, &prod_schema, &collection).await? {
                        skipped.push(json!({ "id": id, "app": app, "source": "collection", "action": action, "collection": collection, "reason": "collection missing in production" }));
                        continue;
                    }
                    let pk_cols = pk_columns(&mut *tx, &prod_schema, &collection).await?;
                    let Some(pk) = (pk_cols.len() == 1).then(|| pk_cols[0].clone()) else {
                        skipped.push(json!({ "id": id, "app": app, "source": "collection", "action": action, "collection": collection, "reason": "composite pk not supported" }));
                        continue;
                    };
                    let Some(key) = edit_key(None, Some(&item_id), &pk) else {
                        skipped.push(json!({ "id": id, "app": app, "source": "collection", "action": action, "collection": collection, "reason": "item_id is not a scalar" }));
                        continue;
                    };
                    match action.as_str() {
                        "item_created" => {
                            // Inverse of a create: delete the row.
                            sqlx::query(&format!(
                                "DELETE FROM {} WHERE {}::text = $1",
                                qtable(&prod_schema, &collection),
                                qident(&pk),
                            ))
                            .bind(&key)
                            .execute(&mut *tx)
                            .await?;
                        }
                        "item_deleted" => {
                            // Inverse of a delete: re-insert the old row built
                            // from diff.*.old.
                            let mut old = serde_json::Map::new();
                            if let Some(obj) = diff.as_object() {
                                for (f, ch) in obj {
                                    old.insert(
                                        f.clone(),
                                        ch.get("old").cloned().unwrap_or(Value::Null),
                                    );
                                }
                            }
                            sqlx::query(&format!(
                                "INSERT INTO {t} OVERRIDING SYSTEM VALUE \
                                 SELECT * FROM jsonb_populate_record(NULL::{t}, $1::jsonb)",
                                t = qtable(&prod_schema, &collection),
                            ))
                            .bind(Value::Object(old))
                            .execute(&mut *tx)
                            .await?;
                        }
                        _ => {
                            // item_updated: set each changed field back to old.
                            let mut fields: Vec<(String, Value)> = Vec::new();
                            if let Some(obj) = diff.as_object() {
                                for (f, ch) in obj {
                                    fields.push((
                                        f.clone(),
                                        ch.get("old").cloned().unwrap_or(Value::Null),
                                    ));
                                }
                            }
                            if fields.is_empty() {
                                skipped.push(json!({ "id": id, "app": app, "source": "collection", "action": action, "collection": collection, "reason": "empty diff" }));
                                continue;
                            }
                            // Column data types of the production table, so
                            // each restored field gets a type-correct SET
                            // expression (a bare `$N::jsonb` assignment only
                            // has an assignment cast for jsonb columns).
                            let col_types =
                                column_data_types(&mut *tx, &prod_schema, &collection).await?;
                            let set_clause = fields
                                .iter()
                                .enumerate()
                                .map(|(i, (f, _))| {
                                    let data_type =
                                        col_types.get(f).map(String::as_str).unwrap_or("jsonb");
                                    revert_set_expr(f, i + 1, data_type)
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            let pk_pos = fields.len() + 1;
                            let update_sql = format!(
                                "UPDATE {} SET {} WHERE {}::text = ${}::text",
                                qtable(&prod_schema, &collection),
                                set_clause,
                                qident(&pk),
                                pk_pos,
                            );
                            let mut q = sqlx::query(&update_sql);
                            for (_, v) in &fields {
                                q = q.bind(v);
                            }
                            q = q.bind(&key);
                            q.execute(&mut *tx).await?;
                        }
                    }
                    reverted.push(json!({ "id": id, "app": app, "source": "collection", "action": action, "collection": collection, "item_id": item_id }));
                }
            }

            // New apps cannot be reverted automatically (dropping a production
            // schema is too destructive to do implicitly).
            for a in &entry.apps {
                skipped.push(json!({ "app": a, "source": "app", "action": "app_created", "reason": "new apps are not reverted automatically" }));
            }

            rewrite_collection_app_columns(&mut tx, &prod_schema)
                .await
                .map_err(|e| {
                    AlcedoError::SystemError(
                        format!("Failed to re-point collection rows in {}: {}", prod_schema, e),
                        0,
                    )
                })?;
            tx.commit().await?;
            reverted_out.insert(
                app.clone(),
                json!({
                    "system_ids": entry.system_ids,
                    "collection_ids": entry.collection_ids,
                    "apps": entry.apps,
                }),
            );
            touched_schemas.push(prod_schema);
        }

        // Audit row; `applied_edits_full_for_version` subtracts these ids again.
        let ctx = AppContext::system(RequestSource::API);
        activity_logs::record(
            self.app_state,
            &ctx,
            "version_reverted",
            &version_name,
            Some(format!(
                "Reverted version '{}' from production (no data backup exists)",
                version_name
            )),
            json!({
                "version_id": version_id,
                "version_name": version_name,
                "applied": reverted_out,
                "warning": NO_BACKUP_WARNING,
            }),
            None,
            None,
            None,
            None,
        )
        .await?;

        if !touched_schemas.is_empty() {
            SchemaService::refresh_schemas_scoped(self.app_state, &touched_schemas).await?;
        }

        Ok(DeployResponse {
            ok: true,
            data: json!({
                "version_id": version_id,
                "version_name": version_name,
                "reverted": reverted,
                "skipped": skipped,
                "warning": NO_BACKUP_WARNING,
            }),
        })
    }
}

/// The SET expression restoring one field of an `item_updated` revert.
///
/// jsonb columns keep the plain `$N::jsonb` assignment. Every other column
/// type is restored through its text representation: a direct
/// `SET col = $N::jsonb` has no assignment cast from jsonb to text/int/...,
/// so it would error and roll back the whole revert transaction. A JSON null
/// restores SQL NULL for non-jsonb columns.
fn revert_set_expr(field: &str, param_pos: usize, data_type: &str) -> String {
    if data_type == "jsonb" {
        format!("{} = ${}::jsonb", qident(field), param_pos)
    } else {
        format!(
            "{} = (CASE WHEN jsonb_typeof(${p}::jsonb) = 'null' THEN NULL \
             ELSE (${p}::jsonb)::text::{t} END)",
            qident(field),
            p = param_pos,
            t = data_type,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::revert_set_expr;

    #[test]
    fn revert_set_expr_keeps_plain_jsonb_cast_for_jsonb_columns() {
        assert_eq!(revert_set_expr("meta", 1, "jsonb"), "\"meta\" = $1::jsonb");
    }

    #[test]
    fn revert_set_expr_casts_through_text_for_non_jsonb_columns() {
        assert_eq!(
            revert_set_expr("title", 2, "text"),
            "\"title\" = (CASE WHEN jsonb_typeof($2::jsonb) = 'null' THEN NULL \
             ELSE ($2::jsonb)::text::text END)"
        );
        assert_eq!(
            revert_set_expr("count", 3, "integer"),
            "\"count\" = (CASE WHEN jsonb_typeof($3::jsonb) = 'null' THEN NULL \
             ELSE ($3::jsonb)::text::integer END)"
        );
        assert_eq!(
            revert_set_expr("created_at", 4, "timestamp with time zone"),
            "\"created_at\" = (CASE WHEN jsonb_typeof($4::jsonb) = 'null' THEN NULL \
             ELSE ($4::jsonb)::text::timestamp with time zone END)"
        );
    }

    #[test]
    fn revert_set_expr_quotes_field_names() {
        assert_eq!(
            revert_set_expr("we\"ird", 1, "jsonb"),
            "\"we\"\"ird\" = $1::jsonb"
        );
    }
}
