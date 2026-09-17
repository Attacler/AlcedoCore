use axum::{
    extract::{Path, State},
    Json,
};
use alcedo_common::RequestIdentity;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::permission_check::{self, PermissionCheck};
use crate::error::AppError;
use crate::events::SystemEvent;
use crate::middleware;
use crate::plugins::health::AppState;

use super::check_relational_permissions;

pub(crate) async fn update_collection_item(
    State(state): State<Arc<AppState>>,
    Path((name, id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Json(body): Json<serde_json::Map<String, Value>>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let pc = permission_check::check_permission(&state, &identity, &headers, &name, "update").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    // Enforce field-level write restrictions from permissions
    if let PermissionCheck::Granted {
        ref permissions, ..
    } = pc
    {
        let update_perms: Vec<_> = permissions
            .iter()
            .filter(|p| p.action == "update")
            .cloned()
            .collect();
        if !update_perms.is_empty()
            && !crate::services::permissions::has_unrestricted_write_access(&update_perms)
        {
            let allowed = crate::services::permissions::get_allowed_write_fields(&update_perms);
            if !allowed.is_empty() {
                for key in body.keys() {
                    if !allowed.contains(key) {
                        return Err(AppError::Forbidden(format!(
                            "Field '{}' is not allowed for update",
                            key
                        )));
                    }
                }
            }
        }
    }

    // Resolve collection schema (validates collection exists -> 404 if missing).
    let collection = crate::db::collections::get_collection(db_pool, &name).await?;
    let all_collections = crate::db::collections::list_collections(db_pool).await?;

    if body.is_empty() {
        return Err(AppError::BadRequest("No fields to update".to_string()));
    }

    // Check cross-collection permissions for relational write operations
    check_relational_permissions(&state, &identity, &headers, &collection, &all_collections, &body).await?;

    let app_ctx = alcedo_common::context::headers_to_app_context(&headers);
    let engine = alcedo_db::services::items::service::ItemsService::new(&state.core, &app_ctx, &name);
    let permissions: Vec<crate::services::permissions::PolicyPermission> = match &pc {
        PermissionCheck::Granted { permissions, .. } => permissions.clone(),
        _ => vec![],
    };
    let outcome = engine.update_one(db_pool, &id, &body, &permissions).await?;
    let updated = outcome.affected.into_iter().next().unwrap_or(Value::Null);
    let old_item = outcome
        .pairs
        .into_iter()
        .next()
        .map(|(old, _)| old)
        .unwrap_or(Value::Null);

    // --- Compute diff and emit ItemUpdated ---
    let diff = crate::events::diff::compute_field_diffs(&old_item, &updated);
    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    state.event_bus.emit(SystemEvent::ItemUpdated {
        collection_name: name.clone(),
        item_id: Value::String(id.clone()),
        old_values: old_item,
        new_values: updated.clone(),
        diff,
        request_id: Some(request_id),
    });
    // --- End event emission ---

    // Apply field-level read restrictions to the response
    let response_item = if let PermissionCheck::Granted {
        ref permissions, ..
    } = pc
    {
        let update_perms: Vec<_> = permissions
            .iter()
            .filter(|p| p.action == "update")
            .cloned()
            .collect();
        if !update_perms.is_empty() {
            crate::api::permission_check::restrict_item_fields(&updated, &update_perms)
        } else {
            updated.clone()
        }
    } else {
        updated.clone()
    };

    Ok(Json(json!({ "updated": response_item })))
}
