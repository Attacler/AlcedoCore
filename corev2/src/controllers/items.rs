use std::collections::HashMap;

use crate::middelware::auth::AuthLevel;
use crate::services::items::service::ItemsService;
use crate::services::items::{query::Query, relational};
use crate::services::permissions::{self, PermissionCheck, PolicyPermission, WriteGuard};
use crate::services::respond::JSendResponse;
use crate::services::respond::success;
use crate::services::{
    context::{AppContext, ExtractContext},
    errors::AlcedoError,
    items::query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp},
    collections::schema::get_pk_key,
    query_parse::CustomQuery,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde_json::{Map, Value, json};

use crate::AppState;

pub fn items_controller() -> Router<AppState> {
    return Router::new()
        .route(
            "/{collection}",
            get(get_items)
                .post(create_items)
                .patch(update_items)
                .delete(delete_items),
        )
        .route("/{collection}/{id}", get(get_item).patch(update_item))
        .route("/{collection}/{id}/references", get(get_references));
}

/// Resolves the caller's rules for an action. `None` = bypass (admin/dev-key).
async fn action_permissions(
    state: &AppState,
    auth_level: &AuthLevel,
    context: &AppContext,
    collection: &str,
    action: &str,
) -> Result<Option<Vec<PolicyPermission>>, AlcedoError> {
    match permissions::check_permission(state, auth_level, context, collection, action).await? {
        PermissionCheck::Bypass => Ok(None),
        PermissionCheck::Granted(perms) => Ok(Some(perms)),
        PermissionCheck::Denied { reason } => Err(AlcedoError::Forbidden(reason, 0)),
    }
}

/// Fetches rows by primary keys, applying read policy when `perms` is present.
async fn read_by_ids(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    ids: Vec<Value>,
    perms: Option<&[PolicyPermission]>,
    auth_level: &AuthLevel,
) -> Result<Vec<Value>, AlcedoError> {
    let pk = get_pk_key(&state.database_schema, &context.schema_name(), collection)
        .await?
        .name;
    let mut hmap = FieldFilter {
        fields: HashMap::new(),
    };
    hmap.fields.insert(
        pk,
        FieldValue::Comparison(Comparison {
            _in: Some(Value::Array(ids)),
            ..Default::default()
        }),
    );
    let query = Query {
        filter: LogicOp {
            _and: Some(vec![Filter::Field(hmap)]),
            _or: None,
        },
        limit: 0,
        ..Default::default()
    };

    match perms {
        None => {
            let table = collection.to_string();
            Ok(ItemsService::new(state, context, &table)
                .read_items_by_query(query)
                .await?
                .into_iter()
                .map(Value::Object)
                .collect())
        }
        Some(perms) => {
            permissions::read_with_permissions(
                state,
                context,
                collection,
                query,
                perms,
                auth_level,
            )
            .await
        }
    }
}

// @TODO Better support for de API explorer/docs. Ticket https://github.com/Authress-Engineering/openapi-explorer/issues/294 describes the issue.
#[derive(utoipa::IntoParams, serde::Deserialize, utoipa::ToSchema)]
#[into_params(style = DeepObject, parameter_in = Query)]
pub struct DocsItemFilter {
    #[serde(rename = "filter")]
    filter: std::collections::HashMap<String, serde_json::Value>,
}

#[utoipa::path(get, path = "/api/app/items/{collection}",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("filter" = String, Query, description = "See \"Items - Query\" for more information"),
        ("fields" = String, Query, description = "See \"Items - Query\" for more information"),
        ("sort" = String, Query, description = "See \"Items - Query\" for more information"),
        ("limit" = String, Query, description = "See \"Items - Query\" for more information"),
        ("page" = String, Query, description = "1-based page number (admin UI)"),
        ("per_page" = String, Query, description = "Page size (admin UI)"),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK)
    )
)]
async fn get_items(
    State(state): State<AppState>,
    CustomQuery(mut query): CustomQuery<Query>,
    Path(collection): Path<String>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    query.normalize_pagination();
    let perms = action_permissions(&state, &auth_level, &context, &collection, "read").await?;

    let service = ItemsService::new(&state, &context, &collection);
    let limit = query.limit;
    let offset = query.offset;

    let (total, items): (i64, Vec<Value>) = match perms {
        None => {
            let total = service.count_items_by_query(query.clone()).await?;
            let items = service
                .read_items_by_query(query)
                .await?
                .into_iter()
                .map(Value::Object)
                .collect();
            (total, items)
        }
        Some(perms) => {
            // Apply the read spec once so both the count and the rows are
            // filtered by the same policy.
            let spec = permissions::build_read_spec(
                &state,
                &context,
                &perms,
                &collection,
                true,
            )
            .await;
            permissions::apply_spec(&mut query, &spec);
            query.read_guard = Some(auth_level.clone());

            let total = service.count_items_by_query(query.clone()).await?;
            let rows = service.read_items_by_query(query).await?;
            let items = rows
                .into_iter()
                .map(|mut row| {
                    let perm = permissions::permissions_from_row(&mut row, &spec);
                    permissions::inject_permissions(&Value::Object(row), perm)
                })
                .collect();
            (total, items)
        }
    };

    Ok(Json(success(json!({
        "data": items,
        "total": total,
        "limit": limit,
        "offset": offset,
    }))))
}

#[utoipa::path(get, path = "/api/app/items/{collection}/{id}",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("id" = String, Path, description = "Item primary key."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK)
    )
)]
async fn get_item(
    State(state): State<AppState>,
    Path((collection, id)): Path<(String, String)>,
    CustomQuery(mut query): CustomQuery<Query>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Map<String, Value>>>, AlcedoError> {
    let perms = action_permissions(&state, &auth_level, &context, &collection, "read").await?;

    let pk = get_pk_key(&state.database_schema, &context.schema_name(), &collection)
        .await?
        .name;
    query.filter = Query::eq(&pk, Value::String(id.clone())).filter;

    let item: Value = match perms {
        None => ItemsService::new(&state, &context, &collection)
            .read_items_by_query(query)
            .await?
            .into_iter()
            .next()
            .map(Value::Object),
        Some(perms) => {
            permissions::read_with_permissions(
                &state,
                &context,
                &collection,
                query,
                &perms,
                &auth_level,
            )
            .await?
            .into_iter()
            .next()
        }
    }
    .ok_or_else(|| AlcedoError::NotFound(format!("Item '{}' not found", id), 1))?;

    let map = item
        .as_object()
        .cloned()
        .ok_or_else(|| AlcedoError::NotFound(format!("Item '{}' not found", id), 1))?;
    Ok(Json(success(map)))
}

#[utoipa::path(post, path = "/api/app/items/{collection}",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body(content = Value, content_type = "application/json", description="A single item object, or an array of item objects."),
    responses(
        (status = OK)
    )
)]
async fn create_items(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(body): Json<Value>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let items: Vec<Map<String, Value>> = match body {
        Value::Array(array) => array
            .into_iter()
            .filter_map(|value| value.as_object().cloned())
            .collect(),
        Value::Object(object) => vec![object],
        _ => {
            return Err(AlcedoError::InvalidInput(
                "Expected a single item object or an array of item objects".to_string(),
                1,
            ));
        }
    };

    let perms = action_permissions(&state, &auth_level, &context, &collection, "create").await?;
    let guard = perms
        .as_ref()
        .map(|_| WriteGuard {
            auth_level: auth_level.clone(),
        });

    let mut transaction = state.database_pool.begin().await?;
    let mut ids: Vec<Value> = Vec::new();
    for item in items {
        let id = relational::create_recursive(
            &state,
            &context,
            &mut transaction,
            collection.clone(),
            item,
            guard.as_ref(),
            None,
        )
        .await?;
        ids.push(Value::String(id));
    }
    transaction.commit().await?;

    let created = read_by_ids(
        &state,
        &context,
        &collection,
        ids,
        perms.as_deref(),
        &auth_level,
    )
    .await?;
    Ok(Json(success(json!({ "created": created }))))
}

#[utoipa::path(patch, path = "/api/app/items/{collection}/{id}",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("id" = String, Path, description = "Item primary key."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body(content = Map<String, Value>, content_type = "application/json", description="Values to update (may include nested relational operations)."),
    responses(
        (status = OK)
    )
)]
async fn update_item(
    State(state): State<AppState>,
    Path((collection, id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(body): Json<Map<String, Value>>,
) -> Result<Json<JSendResponse<Map<String, Value>>>, AlcedoError> {
    let perms = action_permissions(&state, &auth_level, &context, &collection, "update").await?;

    let guard = perms.as_ref().map(|_| WriteGuard {
        auth_level: auth_level.clone(),
    });

    let mut transaction = state.database_pool.begin().await?;
    relational::update_recursive(
        &state,
        &context,
        &mut transaction,
        collection.clone(),
        id.clone(),
        body,
        guard.as_ref(),
    )
    .await?;
    transaction.commit().await?;

    let updated = read_by_ids(
        &state,
        &context,
        &collection,
        vec![Value::String(id.clone())],
        perms.as_deref(),
        &auth_level,
    )
    .await?;
    let map = updated
        .into_iter()
        .next()
        .and_then(|v| v.as_object().cloned())
        .ok_or_else(|| AlcedoError::NotFound(format!("Item '{}' not found", id), 1))?;
    Ok(Json(success(map)))
}

#[utoipa::path(patch, path = "/api/app/items/{collection}",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("filter" = String, Query, description = "Used for choosing the items to update. See \"Items - Query\" for more information"),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body(content = Map<String, Value>, content_type = "application/json", description="Values to update."),
    responses(
        (status = OK)
    )
)]
async fn update_items(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    CustomQuery(mut query): CustomQuery<Query>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(item): Json<Map<String, Value>>,
) -> Result<Json<JSendResponse<Vec<Value>>>, AlcedoError> {
    let perms = action_permissions(&state, &auth_level, &context, &collection, "update").await?;

    if query.filter._and.as_ref().map_or(true, |v| v.is_empty()) {
        return Err(AlcedoError::InvalidInput(
            "A non-empty _and filter is required for updating items.".to_string(),
            1,
        ));
    }

    if let Some(perms) = &perms {
        permissions::apply_action_filter(&mut query, perms, "update");
        permissions::check_write(perms, "update", &item)?;
    }

    let service = ItemsService::new(&state, &context, &collection);
    let result = service
        .update_items_by_query(&mut query.clone(), item, &mut None)
        .await?;

    let rows = read_by_ids(
        &state,
        &context,
        &collection,
        result.into_iter().map(Value::String).collect(),
        perms.as_deref(),
        &auth_level,
    )
    .await?;
    Ok(Json(success(rows)))
}

#[utoipa::path(delete, path = "/api/app/items/{collection}",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("filter" = DocsItemFilter, Query, style = DeepObject, explode, description = "Used for choosing the items to delete. See \"Items - Query\" for more information"),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body(content = Value, content_type = "application/json", description="Optional: { pk_values: [...] } or { filter: {...} }"),
    responses(
        (status = OK)
    )
)]
async fn delete_items(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    CustomQuery(query): CustomQuery<Query>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    body: Option<Json<Value>>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let service = ItemsService::new(&state, &context, &collection);
    let pk = get_pk_key(&state.database_schema, &context.schema_name(), &collection)
        .await?
        .name;

    let perms = action_permissions(&state, &auth_level, &context, &collection, "delete").await?;

    // Resolve the caller's selection into a single query so the policy row
    // filter can be applied uniformly (including pk_values selections).
    let mut query = match body {
        Some(Json(value)) => {
            if let Some(pk_values) = value.get("pk_values").and_then(|v| v.as_array()) {
                let mut hmap = FieldFilter {
                    fields: HashMap::new(),
                };
                hmap.fields.insert(
                    pk.clone(),
                    FieldValue::Comparison(Comparison {
                        _in: Some(Value::Array(pk_values.clone())),
                        ..Default::default()
                    }),
                );
                Query {
                    filter: LogicOp {
                        _and: Some(vec![Filter::Field(hmap)]),
                        _or: None,
                    },
                    ..Default::default()
                }
            } else if let Some(filter) = value.get("filter") {
                serde_json::from_value(json!({ "filter": filter }))
                    .map_err(|e| AlcedoError::InvalidInput(format!("Invalid filter: {}", e), 1))?
            } else {
                return Err(AlcedoError::InvalidInput(
                    "Request body must contain 'pk_values' or 'filter'".to_string(),
                    1,
                ));
            }
        }
        None => {
            if query.filter._and.as_ref().map_or(true, |v| v.is_empty()) {
                return Err(AlcedoError::InvalidInput(
                    "A non-empty _and filter is required for deleting items.".to_string(),
                    1,
                ));
            }
            query
        }
    };

    if let Some(perms) = &perms {
        permissions::apply_action_filter(&mut query, perms, "delete");
    }

    let deleted = service.delete_items_by_query(query, &mut None).await?;
    Ok(Json(success(json!({ "deleted": deleted }))))
}

#[utoipa::path(get, path = "/api/app/items/{collection}/{id}/references",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("id" = String, Path, description = "Item primary key."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK)
    )
)]
async fn get_references(
    State(state): State<AppState>,
    Path((collection, _id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    // TODO: resolve actual back-references. The detail page degrades gracefully.
    action_permissions(&state, &auth_level, &context, &collection, "read").await?;
    Ok(Json(success(json!({ "references": [] }))))
}
