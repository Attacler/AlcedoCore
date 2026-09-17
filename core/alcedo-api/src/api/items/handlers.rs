use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use alcedo_common::context::ExtractContext;
use alcedo_common::RequestIdentity;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::api::collections as collections_api;
use crate::api::permission_check::{self, PermissionCheck};
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

/// Long-form filter node for the collection query endpoint. Supports leaf rules
/// `{"field","operator","value"}` and groups
/// `{"operator":"and"/"or","conditions":[...]}` (the shape emitted by the
/// FilterBuilder frontend component).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LongFormFilter {
    field: Option<String>,
    operator: Option<String>,
    value: Option<Value>,
    combinator: Option<String>,
    conditions: Option<Vec<LongFormFilter>>,
}

/// POST /api/items/:slug/query body for collections (public-schema tables).
/// Deliberately rejects unknown keys (e.g. the plugin-style `filters`/`select`
/// keys) so a mismatched payload surfaces as a 422 instead of silently
/// returning unfiltered rows.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionQueryRequest {
    filter: Option<LongFormFilter>,
    fields: Option<Vec<String>>,
    sort: Option<Vec<crate::db::filter_condition::SortField>>,
    limit: Option<u64>,
    offset: Option<u64>,
    #[serde(default)]
    backlink: bool,
}

/// Convert a long-form filter node into the short-form `FilterCondition` tree
/// understood by `filter_compiler::compile_filter`.
fn long_form_to_filter_node(
    f: &LongFormFilter,
) -> Result<crate::db::filter_condition::FilterCondition, AppError> {
    use crate::db::filter_condition::{ComparisonOperator, FilterCondition, LogicOperator};

    if let Some(conds) = &f.conditions {
        let operator = match f.combinator.as_deref().or(f.operator.as_deref()) {
            Some("and") => LogicOperator::And,
            Some("or") => LogicOperator::Or,
            _ => {
                return Err(AppError::BadRequest(
                    "Filter group requires an 'and' or 'or' operator".to_string(),
                ))
            }
        };
        let conditions = conds
            .iter()
            .map(long_form_to_filter_node)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(FilterCondition::Group {
            operator,
            conditions,
        });
    }

    let field = f
        .field
        .clone()
        .ok_or_else(|| AppError::BadRequest("Filter rule is missing 'field'".to_string()))?;
    let op_str = f.operator.clone().ok_or_else(|| {
        AppError::BadRequest(format!("Filter rule for '{}' is missing 'operator'", field))
    })?;
    let operator = match op_str.as_str() {
        "eq" => ComparisonOperator::Eq,
        "neq" | "not_eq" => ComparisonOperator::Neq,
        "gt" => ComparisonOperator::Gt,
        "gte" => ComparisonOperator::Gte,
        "lt" => ComparisonOperator::Lt,
        "lte" => ComparisonOperator::Lte,
        "contains" => ComparisonOperator::Contains,
        "starts_with" => ComparisonOperator::StartsWith,
        "ends_with" => ComparisonOperator::EndsWith,
        "in" => ComparisonOperator::In,
        "not_in" => ComparisonOperator::NotIn,
        "null" | "is_null" => ComparisonOperator::IsNull,
        "not_null" | "is_not_null" => ComparisonOperator::IsNotNull,
        other => {
            return Err(AppError::BadRequest(format!(
                "Unknown filter operator '{}'",
                other
            )))
        }
    };

    Ok(FilterCondition::Rule {
        field,
        operator,
        value: f.value.clone(),
    })
}

pub(crate) async fn reject_system_collection(
    pool: &sqlx::PgPool,
    redis: &Option<std::sync::Arc<crate::services::redis_client::RedisClient>>,
    schema: &str,
    slug: &str,
) -> Result<(), AppError> {
    if let Ok(col) = collections::get_cached_collection(pool, redis, schema, slug).await {
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
    identity: RequestIdentity,
    Json(request): Json<GroupedQueryRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let pc = permission_check::check_permission(&state, &identity, &headers, &collection_name, "read").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    let extra_permissions: &[crate::services::permissions::PolicyPermission] = match &pc {
        PermissionCheck::Granted { permissions, .. } => permissions.as_slice(),
        _ => &[],
    };

    let app_ctx = alcedo_common::context::headers_to_app_context(&headers);
    let engine = alcedo_db::services::items::service::ItemsService::new(
        &state.core,
        &app_ctx,
        &collection_name,
    );
    let response = engine.read_grouped(db_pool, request, extra_permissions).await?;

    let all_perms = permission_check::load_all_user_permissions(&state, &identity, &headers, &collection_name).await?;
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
    identity: RequestIdentity,
    ExtractContext(ctx): ExtractContext,
    body: Json<Value>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    let schema = state.schema_for_headers(&headers).await?;

    reject_system_collection(db_pool, &state.redis, &schema, &slug).await?;

    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    let start = Instant::now();

    // Permission check before query execution so we inject filters at the SQL level
    let pc = permission_check::check_permission(&state, &identity, &headers, &slug, "read").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    // Collections live in the public schema; plugins in their own `plugin_{slug}` schema.
    let is_collection =
        collections::get_cached_collection(db_pool, &state.redis, &schema, &slug).await.is_ok();

    let (response_json, rows_len, columns_len) = if is_collection {
        // --- Collection path: advanced query against the public-schema table ---
        let collection_request: CollectionQueryRequest = serde_json::from_value(body.0).map_err(
            |e| AppError::UnprocessableEntity(format!("Invalid query request body: {}", e)),
        )?;

        let filter_cond = match &collection_request.filter {
            Some(f) => Some(long_form_to_filter_node(f)?),
            None => None,
        };
        let permissions: Vec<crate::services::permissions::PolicyPermission> = match &pc {
            PermissionCheck::Granted { permissions, .. } => permissions.clone(),
            _ => vec![],
        };
        let app_ctx = alcedo_common::context::headers_to_app_context(&headers);
        let engine = alcedo_db::services::items::service::ItemsService::new(
            &state.core,
            &app_ctx,
            &slug,
        );
        let result = engine
            .read_list(
                db_pool,
                alcedo_db::services::items::read::ListRequest {
                    fields: collection_request.fields.clone().unwrap_or_default(),
                    filter: filter_cond,
                    sort: collection_request.sort.clone().unwrap_or_default(),
                    limit: collection_request.limit.unwrap_or(100),
                    offset: collection_request.offset.unwrap_or(0),
                    backlink: collection_request.backlink,
                    depth_limit: state.nested_field_depth_limit,
                    permissions,
                    augment: false,
                },
            )
            .await?;

        let total = result.total;
        let rows = result.items;

        let all_perms =
            permission_check::load_all_user_permissions(&state, &identity, &headers, &slug).await?;
        let is_admin = pc.permissions().is_none();
        let rows: Vec<Value> = rows
            .into_iter()
            .map(|row| {
                let perms =
                    permission_check::compute_item_permissions_sync(&row, &all_perms, is_admin);
                permission_check::inject_permissions(&row, perms)
            })
            .collect();
        let rows_len = rows.len();
        (serde_json::json!({ "data": rows, "total": total }), rows_len, 0)
    } else {
        // --- Plugin path: plugin-schema tables ---
        let mut request: QueryRequest = serde_json::from_value(body.0).map_err(|e| {
            AppError::UnprocessableEntity(format!("Invalid query request body: {}", e))
        })?;

        // Scope the plugin schema to the install resolved for this request so
        // app/version installs never read the global schema.
        let (app_version_id, version_id) =
            crate::api::install::resolve_install_scope_for_context(db_pool, &slug, &ctx).await?;

        if let PermissionCheck::Granted { ref permissions, .. } = pc {
            if !permissions.is_empty() {
                // Resolve table schema to get column names for field expressions (plugin schemas only)
                let schema_name = crate::db::plugin_migrations::plugin_schema_name_for_scope(
                    &slug,
                    app_version_id,
                    version_id,
                );
                let schemas = crate::db::schema::get_table_schemas(db_pool, &schema_name).await?;
                let table_schema = schemas
                    .into_iter()
                    .find(|t| t.table_name != "_sqlx_migrations")
                    .ok_or_else(|| AppError::NotFound(format!("No user tables for plugin '{}'", &slug)))?;
                let all_field_names: Vec<String> = table_schema
                    .columns
                    .iter()
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

        let mut response =
            query_items(db_pool, &slug, request, app_version_id, version_id).await?;

        let all_perms =
            permission_check::load_all_user_permissions(&state, &identity, &headers, &slug).await?;
        let is_admin = pc.permissions().is_none();
        response.rows = response
            .rows
            .into_iter()
            .map(|row| {
                let perms =
                    permission_check::compute_item_permissions_sync(&row, &all_perms, is_admin);
                permission_check::inject_permissions(&row, perms)
            })
            .collect();
        let rows_len = response.row_count;
        let columns_len = response.columns.len();
        (serde_json::json!(response), rows_len, columns_len)
    };

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
            format!("columns: {}, rows: {}", columns_len, rows_len),
            duration_ms as i32,
        );
    }

    Ok(Json(response_json))
}
