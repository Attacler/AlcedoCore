use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::api::collections as collections_api;
use crate::api::permission_check::{self, PermissionCheck};
use crate::db::collection_items::{self};
use crate::db::collections::{self};
use crate::db::filter_condition::GroupedQueryRequest;
use crate::db::items::query_items;
use crate::db::query_builder::QueryRequest;
use crate::db::query_builder::FilterCondition as QueryFilterCondition;
use crate::error::AppError;
use crate::middleware;
use crate::plugins::health::AppState;
use crate::services::permissions as permissions_service;

use crate::api::items::detail::{get_item_handler, references_handler};
use crate::api::items::listing::list_items_handler;
use crate::api::items::mutations::{create_handler, update_handler, delete_handler};
use crate::api::items::filters::permissions_to_query_filter;

pub(crate) async fn reject_system_collection(
    pool: &sqlx::PgPool,
    redis: &Option<crate::services::redis_session::RedisPool>,
    slug: &str,
) -> Result<(), AppError> {
    if let Ok(col) = collections::get_cached_collection(pool, redis, slug).await {
        if col.is_system {
            return Err(AppError::NotFound(format!("'{}' is a system collection — use its dedicated management endpoint instead", slug)));
        }
    }
    Ok(())
}

pub fn items_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/items/:slug/query", post(query_handler))
        .route("/api/items/:slug/grouped", post(grouped_handler))
        .route("/api/items/:slug/:id/references", get(references_handler))
        .route("/api/items/:slug/:id", get(get_item_handler).patch(collections_api::update_collection_item))
        .route("/api/items/:slug", get(list_items_handler).post(create_handler).put(update_handler).delete(delete_handler))
        .with_state(state)
}

async fn grouped_handler(
    State(state): State<Arc<AppState>>,
    Path(collection_name): Path<String>,
    headers: axum::http::HeaderMap,
    Json(request): Json<GroupedQueryRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let pc = permission_check::check_permission(&state, &headers, &collection_name, "read").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    let extra_permissions: &[crate::services::permissions::PolicyPermission] = match &pc {
        PermissionCheck::Granted { permissions, .. } => permissions.as_slice(),
        _ => &[],
    };

    let response = collection_items::grouped_query_items(
        db_pool, &collection_name, request, extra_permissions,
    ).await?;

    let all_perms = permission_check::load_all_user_permissions(&state, &headers, &collection_name).await?;
    let _is_admin = pc.permissions().is_none();

    let restricted = match &pc {
        PermissionCheck::Granted { permissions, .. } => {
            let restricted_groups: Vec<crate::db::filter_condition::GroupResult> = response.groups.into_iter().map(|mut g| {
                g.items = g.items.into_iter().map(|item| {
                    let r = permission_check::restrict_item_fields(&item, permissions);
                    let perms = permission_check::compute_item_permissions_sync(&r, &all_perms, false);
                    permission_check::inject_permissions(&r, perms)
                }).collect();
                g.count = g.items.len() as i64;
                g
            }).collect();
            let total = restricted_groups.len() as i64;
            crate::db::filter_condition::GroupedQueryResponse {
                groups: restricted_groups,
                total,
            }
        }
        _ => {
            let bypass_groups: Vec<crate::db::filter_condition::GroupResult> = response.groups.into_iter().map(|mut g| {
                g.items = g.items.into_iter().map(|item| {
                    let perms = permission_check::compute_item_permissions_sync(&item, &[], true);
                    permission_check::inject_permissions(&item, perms)
                }).collect();
                g
            }).collect();
            let total = bypass_groups.len() as i64;
            crate::db::filter_condition::GroupedQueryResponse {
                groups: bypass_groups,
                total,
            }
        }
    };

    Ok(Json(json!(restricted)))
}

async fn query_handler(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    headers: axum::http::HeaderMap,
    mut request: Json<QueryRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    reject_system_collection(db_pool, &state.redis_connection, &slug).await?;

    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    let start = Instant::now();

    // Permission check before query execution so we inject filters at the SQL level
    let pc = permission_check::check_permission(&state, &headers, &slug, "read").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }
    if let PermissionCheck::Granted { ref permissions, .. } = pc {
        // For collection queries, the schema is in public schema — skip plugin table lookup
        let is_collection = crate::db::collections::get_cached_collection(db_pool, &state.redis_connection, &slug).await.is_ok();
        if !is_collection && !permissions.is_empty() {
            // Resolve table schema to get column names for field expressions (plugin schemas only)
            let schema_name = crate::db::plugin_migrations::plugin_schema_name(&slug);
            let schemas = crate::db::schema::get_table_schemas(db_pool, &schema_name).await?;
            let table_schema = schemas.into_iter()
                .find(|t| t.table_name != "_sqlx_migrations")
                .ok_or_else(|| AppError::NotFound(format!("No user tables for plugin '{}'", &slug)))?;
            let all_field_names: Vec<String> = table_schema.columns.iter()
                .map(|c| c.column_name.clone())
                .collect();

            // Convert permission filters to structured filter and combine with user's filter
            let perm_filter = permissions_to_query_filter(permissions);
            request.filters = match request.filters.take() {
                Some(existing) => Some(QueryFilterCondition {
                    field: None,
                    operator: None,
                    value: None,
                    combinator: Some("and".to_string()),
                    conditions: Some(vec![existing, perm_filter]),
                }),
                None => Some(perm_filter),
            };

            // Build field-level CASE WHEN expressions for SELECT
            let (field_exprs, _always_allowed, extra_binds) =
                permissions_service::build_field_expressions(permissions, &all_field_names, None);
            request.extra_select = Some(field_exprs);
            if !extra_binds.is_empty() {
                request.extra_select_binds = Some(extra_binds);
            }
        }
    }

    let mut response = query_items(db_pool, &slug, request.0).await?;

    let all_perms = permission_check::load_all_user_permissions(&state, &headers, &slug).await?;
    let is_admin = pc.permissions().is_none();
    response.rows = response.rows.into_iter().map(|row| {
        let perms = permission_check::compute_item_permissions_sync(&row, &all_perms, is_admin);
        permission_check::inject_permissions(&row, perms)
    }).collect();

    let duration_ms = start.elapsed().as_millis() as i64;

    if let Some(ref logging_channel) = state.logging_channel {
        middleware::logging::log_request(
            logging_channel,
            request_id.clone(),
            slug.clone(),
            "POST".to_string(),
            format!("api/items/{}/query", slug),
            200,
            duration_ms,
            middleware::logging::extract_client_ip_from_headers(&headers),
            None,
            None,
            None,
            None,
        ).await;
    }

    if let Some(ref channel) = state.host_call_channel {
        middleware::host_calls::record_host_call(
            channel,
            Some(request_id),
            middleware::host_calls::ActionType::DbQuery,
            format!("items query, slug: {}", slug),
            format!("columns: {}, rows: {}", response.columns.len(), response.row_count),
            duration_ms as i32,
        );
    }

    Ok(Json(serde_json::json!(response)))
}
