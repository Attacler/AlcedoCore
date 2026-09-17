use axum::{
    extract::{Path, Query, State},
    Json,
};
use alcedo_common::RequestIdentity;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::permission_check::{self, PermissionCheck};
use crate::db::filter_condition::{FilterCondition, SortField};
use crate::error::AppError;
use crate::plugins::health::AppState;

use crate::api::items::reject_system_collection;

pub async fn list_items_handler(
    State(state): State<Arc<AppState>>,
    Path(collection_name): Path<String>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    let schema = state.schema_for_headers(&headers).await?;

    reject_system_collection(db_pool, &state.redis, &schema, &collection_name).await?;

    let (limit, offset) = {
        let per_page = params.get("per_page")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100);
        let page = params.get("page")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1);
        let explicit_limit = params.get("limit").and_then(|v| v.parse::<u64>().ok());
        let explicit_offset = params.get("offset").and_then(|v| v.parse::<u64>().ok());
        (
            explicit_limit.unwrap_or(per_page),
            explicit_offset.unwrap_or((page.saturating_sub(1)) * per_page),
        )
    };

    let pc = permission_check::check_permission(&state, &identity, &headers, &collection_name, "read").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    let filter_cond: Option<FilterCondition> = params.get("filter")
        .and_then(|f| serde_json::from_str::<FilterCondition>(f).ok());

    let fields_param: Vec<String> = params.get("fields")
        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();

    let permissions: Vec<crate::services::permissions::PolicyPermission> = match &pc {
        PermissionCheck::Granted { permissions, .. } => permissions.clone(),
        _ => vec![],
    };

    let app_ctx = alcedo_common::context::headers_to_app_context(&headers);
    let engine = alcedo_db::services::items::service::ItemsService::new(
        &state.core,
        &app_ctx,
        &collection_name,
    );
    let result = engine
        .read_list(
            db_pool,
            alcedo_db::services::items::read::ListRequest {
                fields: fields_param,
                filter: filter_cond,
                sort: match params.get("sort").filter(|v| !v.is_empty()) {
                    Some(sf) => vec![SortField {
                        field: sf.clone(),
                        order: params.get("order").map(|v| v.as_str()).unwrap_or("asc").to_string(),
                    }],
                    None => vec![],
                },
                limit,
                offset,
                backlink: true,
                depth_limit: 5,
                permissions,
                augment: true,
            },
        )
        .await?;

    // --- unchanged $permissions injection ---
    let all_perms = permission_check::load_all_user_permissions(&state, &identity, &headers, &collection_name).await?;
    let is_admin = pc.permissions().is_none();
    let mut items_with_perms: Vec<Value> = Vec::new();
    for item in result.items {
        let perms = permission_check::compute_item_permissions(&item, &all_perms, is_admin, Some(db_pool), Some(&collection_name)).await
            .unwrap_or(json!({ "update": false, "delete": false }));
        items_with_perms.push(permission_check::inject_permissions(&item, perms));
    }

    Ok(Json(json!({
        "items": items_with_perms,
        "limit": limit,
        "offset": offset,
        "total": result.total,
    })))
}
