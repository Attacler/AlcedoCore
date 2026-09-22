use std::collections::HashMap;

use crate::services::items::service::ItemsService;
use crate::services::respond::JSendResponse;
use crate::services::respond::success;
use crate::services::{
    context::ExtractContext,
    errors::AlcedoError,
    items::query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query},
    postgres::tables::get_pk_key,
    query_parse::CustomQuery,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde_json::{Map, Value};

use crate::AppState;

pub fn items_controller() -> Router<AppState> {
    return Router::new().route(
        "/{collection}",
        get(get_items)
            .post(create_items)
            .patch(update_items)
            .delete(delete_items),
    );
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
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK)
    )
)]
async fn get_items(
    State(state): State<AppState>,
    CustomQuery(query): CustomQuery<Query>,
    Path(collection): Path<String>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Vec<serde_json::Map<std::string::String, Value>>>>, AlcedoError> {
    let service = ItemsService::new(&state, &context, &collection);
    let result = service.read_items_by_query(query).await?;

    Ok(Json(success(result)))
}

#[utoipa::path(post, path = "/api/app/items/{collection}",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("fields" = String, Query, description = "See \"Items - Query\" for more information."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body(content = Vec<Map<String, Value>>, content_type = "application/json"),
    responses(
        (status = OK)
    )
)]
async fn create_items(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    CustomQuery(mut query): CustomQuery<Query>,
    ExtractContext(context): ExtractContext,
    Json(items): Json<Vec<Map<String, Value>>>,
) -> Result<Json<JSendResponse<Vec<serde_json::Map<std::string::String, Value>>>>, AlcedoError> {
    let service = ItemsService::new(&state, &context, &collection);
    let result = service.create_many(items, &mut None).await?;

    let mut hmap = FieldFilter {
        fields: HashMap::new(),
    };
    let pk = get_pk_key(&state.database_schema, &collection)
        .await
        .unwrap()
        .name;
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

#[utoipa::path(patch, path = "/api/app/items/{collection}",
    params(
        ("collection" = String, Path, description = "Collection name."),
        ("filter" = String, Query, description = "Used for choosing the items to update. See \"Items - Query\" for more information"),
        ("fields" = String, Query, description = "Fields to return. See \"Items - Query\" for more information."),
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
) -> Result<Json<JSendResponse<Vec<serde_json::Map<std::string::String, Value>>>>, AlcedoError> {
    if let None = query.filter._and {
        return Err(AlcedoError::InvalidInput(
            "A _and filter is required for updating items.".to_string(),
            1,
        ));
    }
    let service = ItemsService::new(&state, &context, &collection);
    let result = service
        .update_items_by_query(&mut query.clone(), item, &mut None)
        .await?;

    let pk = get_pk_key(&state.database_schema, &collection)
        .await
        .unwrap()
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
    responses(
        (status = OK)
    )
)]
async fn delete_items(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    CustomQuery(query): CustomQuery<Query>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<u64>>, AlcedoError> {
    if let None = query.filter._and {
        return Err(AlcedoError::InvalidInput(
            "A _and filter is required for deleting items.".to_string(),
            1,
        ));
    }
    let service = ItemsService::new(&state, &context, &collection);
    let result = service.delete_items_by_query(query, &mut None).await?;

    Ok(Json(success(result)))
}
