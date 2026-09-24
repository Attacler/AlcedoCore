use std::collections::HashMap;

use crate::services::items::service::ItemsService;
use crate::services::items::{query::Query, relational};
use crate::services::respond::JSendResponse;
use crate::services::respond::success;
use crate::services::{
    context::ExtractContext,
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
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    query.normalize_pagination();
    let service = ItemsService::new(&state, &context, &collection);
    let total = service.count_items_by_query(query.clone()).await?;
    let limit = query.limit;
    let offset = query.offset;
    let items = service.read_items_by_query(query).await?;

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
) -> Result<Json<JSendResponse<Map<String, Value>>>, AlcedoError> {
    let service = ItemsService::new(&state, &context, &collection);
    let pk = get_pk_key(&state.database_schema, &context.schema_name(), &collection)
        .await?
        .name;

    query.filter = Query::eq(&pk, Value::String(id.clone())).filter;
    let item = service
        .read_items_by_query(query)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AlcedoError::NotFound(format!("Item '{}' not found", id), 1))?;

    Ok(Json(success(item)))
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

    let mut transaction = state.database_pool.begin().await?;
    let mut ids: Vec<Value> = Vec::new();
    for item in items {
        let id = relational::create_recursive(
            &state,
            &context,
            &mut transaction,
            collection.clone(),
            item,
        )
        .await?;
        ids.push(Value::String(id));
    }
    transaction.commit().await?;

    let service = ItemsService::new(&state, &context, &collection);
    let created = service.get_items_by_pks(ids).await?;
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
    Json(body): Json<Map<String, Value>>,
) -> Result<Json<JSendResponse<Map<String, Value>>>, AlcedoError> {
    let mut transaction = state.database_pool.begin().await?;
    relational::update_recursive(
        &state,
        &context,
        &mut transaction,
        collection.clone(),
        id.clone(),
        body,
    )
    .await?;
    transaction.commit().await?;

    let service = ItemsService::new(&state, &context, &collection);
    let item = service
        .get_items_by_pks(vec![Value::String(id.clone())])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AlcedoError::NotFound(format!("Item '{}' not found", id), 1))?;
    Ok(Json(success(item)))
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
    Json(item): Json<Map<String, Value>>,
) -> Result<Json<JSendResponse<Vec<Map<String, Value>>>>, AlcedoError> {
    if query.filter._and.as_ref().map_or(true, |v| v.is_empty()) {
        return Err(AlcedoError::InvalidInput(
            "A non-empty _and filter is required for updating items.".to_string(),
            1,
        ));
    }
    let service = ItemsService::new(&state, &context, &collection);
    let result = service
        .update_items_by_query(&mut query.clone(), item, &mut None)
        .await?;

    let pk = get_pk_key(&state.database_schema, &context.schema_name(), &collection)
        .await?
        .name;
    let mut hmap = FieldFilter {
        fields: HashMap::new(),
    };
    hmap.fields.insert(
        pk,
        FieldValue::Comparison(Comparison {
            _in: Some(result.into()),
            ..Default::default()
        }),
    );
    query.filter = LogicOp {
        _and: Some(vec![Filter::Field(hmap)]),
        _or: None,
    };

    let result = service.read_items_by_query(query).await?;
    Ok(Json(success(result)))
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
    body: Option<Json<Value>>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let service = ItemsService::new(&state, &context, &collection);

    let deleted = match body {
        Some(Json(value)) => {
            if let Some(pk_values) = value.get("pk_values").and_then(|v| v.as_array()) {
                service.delete_items_by_pks(pk_values.clone(), None).await?
            } else if let Some(filter) = value.get("filter") {
                let query: Query = serde_json::from_value(json!({ "filter": filter }))
                    .map_err(|e| AlcedoError::InvalidInput(format!("Invalid filter: {}", e), 1))?;
                service.delete_items_by_query(query, &mut None).await?
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
            service.delete_items_by_query(query, &mut None).await?
        }
    };

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
    State(_state): State<AppState>,
    Path((_collection, _id)): Path<(String, String)>,
    ExtractContext(_context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    // TODO: resolve actual back-references. The detail page degrades gracefully.
    Ok(Json(success(json!({ "references": [] }))))
}
