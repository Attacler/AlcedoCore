use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
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
use crate::api::items::filters::{filter_has_dot_path, permissions_to_query_filter};

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

/// Collection-aware query for `POST /api/items/:slug/query`.
///
/// Resolves the collection definition (public-schema table) and supports
/// long-form filters (with dot-notation JOINs), nested field selection,
/// sorting, and pagination. Returns the matching rows and the total count.
async fn query_collection_items(
    state: &Arc<AppState>,
    collection_name: &str,
    request: CollectionQueryRequest,
    pc: &PermissionCheck,
) -> Result<(Vec<Value>, i64), AppError> {
    let db_pool = state.db()?;

    let collection =
        collections::get_cached_collection(db_pool, &state.redis_connection, collection_name)
            .await?;
    let col_type_map: std::collections::HashMap<&str, &crate::db::collections::FieldType> =
        collection
            .fields
            .iter()
            .map(|f| (f.name.as_str(), &f.field_type))
            .collect();

    let filter_cond = match &request.filter {
        Some(f) => Some(long_form_to_filter_node(f)?),
        None => None,
    };
    let has_dot_notation = filter_cond.as_ref().map_or(false, |fc| filter_has_dot_path(fc));
    let all_collections =
        Some(collections::get_cached_collections(db_pool, &state.redis_connection).await?);

    let mut bind_values: Vec<Value> = Vec::new();
    let mut joins: Vec<String> = Vec::new();

    let mut where_clause = match filter_cond {
        Some(ref filter) => {
            let clause = if has_dot_notation {
                crate::db::filter_compiler::compile_filter_with_joins(
                    filter,
                    &col_type_map,
                    &mut bind_values,
                    collection_name,
                    all_collections.as_ref().unwrap(),
                    &mut joins,
                )?
            } else {
                crate::db::filter_compiler::compile_filter(filter, &col_type_map, &mut bind_values)?
            };
            if clause.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", clause)
            }
        }
        None => String::new(),
    };

    // Inject row-level permission filter (only rows matching a policy rule).
    if let PermissionCheck::Granted { ref permissions, .. } = pc {
        let table_prefix = if joins.is_empty() {
            None
        } else {
            Some(collection_name)
        };
        let (perm_where, perm_binds, perm_joins) = if let Some(ref all_cols) = all_collections {
            permissions_service::build_filter_clause_with_joins(
                permissions,
                bind_values.len() as usize,
                table_prefix,
                collection_name,
                &collection,
                all_cols,
            )
        } else {
            let (w, b) = permissions_service::build_filter_clause_with_offset(
                permissions,
                bind_values.len() as usize,
                table_prefix,
                Some(&collection),
            );
            (w, b, vec![])
        };
        if !perm_where.is_empty() {
            if !perm_joins.is_empty() {
                joins.extend(perm_joins);
            }
            if where_clause.is_empty() {
                where_clause = format!(" WHERE {}", perm_where);
            } else {
                where_clause = format!("{} AND ({})", where_clause, perm_where);
            }
            for val in perm_binds {
                bind_values.push(val);
            }
        }
    }

    let join_clause = if joins.is_empty() {
        String::new()
    } else {
        format!(" {}", joins.join(" "))
    };

    let order_clause = crate::db::filter_compiler::build_order_by(request.sort.as_deref().unwrap_or(&[]));

    // Build SELECT expression with field-level permission restrictions + nested fields.
    let implicit_cols = ["id", "created_at", "updated_at"];
    let mut all_field_names: Vec<String> = collection
        .fields
        .iter()
        .filter(|f| f.relationship_type.as_deref() != Some("one_to_many"))
        .map(|f| f.name.clone())
        .collect();
    for col in &implicit_cols {
        if !all_field_names.contains(&col.to_string()) {
            all_field_names.push(col.to_string());
        }
    }

    let fields_param = request.fields.clone().unwrap_or_default();
    let fields_has_dot_notation = fields_param.iter().any(|f| f.contains('.'));

    let select_expr = if !fields_param.is_empty() {
        let flat_fields: Vec<&String> = fields_param.iter().filter(|f| !f.contains('.')).collect();
        for field_name in &flat_fields {
            if !all_field_names.iter().any(|n| n == *field_name) {
                return Err(AppError::BadRequest(format!(
                    "Unknown field: '{}'",
                    field_name
                )));
            }
        }
        let mut base_cols: Vec<String>;
        if flat_fields.is_empty() {
            if let PermissionCheck::Granted { ref permissions, .. } = pc {
                let table_prefix = if joins.is_empty() {
                    None
                } else {
                    Some(collection_name)
                };
                let (field_exprs, _, mut extra_binds) = permissions_service::build_field_expressions(
                    permissions, &all_field_names, table_prefix,
                );
                base_cols = field_exprs;
                if !extra_binds.is_empty() {
                    extra_binds.extend(bind_values);
                    bind_values = extra_binds;
                }
            } else {
                base_cols = all_field_names
                    .iter()
                    .map(|n| {
                        if joins.is_empty() {
                            format!("\"{}\"", n)
                        } else {
                            format!("\"{}\".\"{}\"", collection_name, n)
                        }
                    })
                    .collect();
            }
        } else {
            let mut names: Vec<String> = flat_fields.iter().map(|s| (*s).clone()).collect();
            for col in &implicit_cols {
                if !names.contains(&col.to_string()) {
                    names.push(col.to_string());
                }
            }
            if let PermissionCheck::Granted { ref permissions, .. } = pc {
                let table_prefix = if joins.is_empty() {
                    None
                } else {
                    Some(collection_name)
                };
                let (field_exprs, _, mut extra_binds) =
                    permissions_service::build_field_expressions(permissions, &names, table_prefix);
                base_cols = field_exprs;
                if !extra_binds.is_empty() {
                    extra_binds.extend(bind_values);
                    bind_values = extra_binds;
                }
            } else {
                base_cols = names
                    .iter()
                    .map(|n| {
                        if joins.is_empty() {
                            format!("\"{}\"", n)
                        } else {
                            format!("\"{}\".\"{}\"", collection_name, n)
                        }
                    })
                    .collect();
            }
        }
        if fields_has_dot_notation {
            let all_collections_ref = all_collections
                .as_ref()
                .ok_or_else(|| AppError::Internal("Collections not loaded".to_string()))?;
            let mut field_opts = crate::db::field_resolver::FieldResolverOptions {
                depth_limit: state.nested_field_depth_limit,
                backlink: request.backlink,
                visited: std::collections::HashSet::new(),
            };
            let fragments = crate::db::field_resolver::resolve_nested_fields(
                &fields_param,
                collection_name,
                all_collections_ref,
                &mut field_opts,
            )?;
            for fragment in &fragments {
                base_cols.push(fragment.select_clause.clone());
            }
        }
        base_cols.join(", ")
    } else if let PermissionCheck::Granted { ref permissions, .. } = pc {
        let table_prefix = if joins.is_empty() {
            None
        } else {
            Some(collection_name)
        };
        let (field_exprs, _always_allowed, mut extra_binds) = permissions_service::build_field_expressions(
            permissions, &all_field_names, table_prefix,
        );
        if !extra_binds.is_empty() {
            extra_binds.extend(bind_values);
            bind_values = extra_binds;
        }
        field_exprs.join(", ")
    } else if joins.is_empty() {
        "*".to_string()
    } else {
        format!("\"{}\".*", collection_name)
    };

    let limit = request.limit.unwrap_or(100);
    let offset = request.offset.unwrap_or(0);

    let data_sql = format!(
        "SELECT COALESCE(json_agg(\"_q\"), '[]'::json) FROM (SELECT {} FROM \"{}\"{}{}{} LIMIT {} OFFSET {}) AS \"_q\"",
        select_expr, collection_name, join_clause, where_clause, order_clause, limit, offset
    );
    let count_sql = format!(
        "SELECT COUNT(*) FROM \"{}\"{}{}",
        collection_name, join_clause, where_clause
    );

    let mut data_q = sqlx::query_as::<_, (Value,)>(&data_sql);
    for val in &bind_values {
        data_q = crate::bind_json_value!(data_q, val);
    }
    let (items_result,): (Value,) = data_q.fetch_one(db_pool).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Collection items query failed: {}", e),
        }
    })?;
    let items = match items_result {
        Value::Array(arr) => arr,
        _ => vec![],
    };

    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values {
        count_q = crate::bind_json_value!(count_q, val);
    }
    let total = count_q.fetch_one(db_pool).await.map_err(|e| {
        AppError::DatabaseError {
            details: format!("Collection count query failed: {}", e),
        }
    })?;

    Ok((items, total))
}

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
    body: Json<Value>,
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

    // Collections live in the public schema; plugins in their own `plugin_{slug}` schema.
    let is_collection =
        collections::get_cached_collection(db_pool, &state.redis_connection, &slug).await.is_ok();

    let (response_json, rows_len, columns_len) = if is_collection {
        // --- Collection path: advanced query against the public-schema table ---
        let collection_request: CollectionQueryRequest = serde_json::from_value(body.0).map_err(
            |e| {
                AppError::UnprocessableEntity(format!("Invalid query request body: {}", e))
            },
        )?;

        let (rows, total) =
            query_collection_items(&state, &slug, collection_request, &pc).await?;

        let all_perms =
            permission_check::load_all_user_permissions(&state, &headers, &slug).await?;
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
        // --- Plugin path: plugin-schema tables (unchanged behavior) ---
        let mut request: QueryRequest = serde_json::from_value(body.0).map_err(|e| {
            AppError::UnprocessableEntity(format!("Invalid query request body: {}", e))
        })?;

        if let PermissionCheck::Granted { ref permissions, .. } = pc {
            if !permissions.is_empty() {
                // Resolve table schema to get column names for field expressions (plugin schemas only)
                let schema_name = crate::db::plugin_migrations::plugin_schema_name(&slug);
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

        let mut response = query_items(db_pool, &slug, request).await?;

        let all_perms =
            permission_check::load_all_user_permissions(&state, &headers, &slug).await?;
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
