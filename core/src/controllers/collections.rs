use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post, put},
};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    controllers::require_admin,
    middelware::auth::AuthLevel,
    services::{
        collections::{
            self as collections_service, CollectionResponse, CreateCollectionRequest,
            SectionRequest, UpdateCollectionRequest,
        },
        context::ExtractContext,
        errors::AlcedoError,
        postgres::{columntypetots::column_type_to_ts, inspector::DatabaseSchema},
        respond::{JSendResponse, success},
        saved_views::{
            CreateSavedViewRequest, SavedViewsService, SavedViewResponse, UpdateSavedViewRequest,
        },
        scopes::require_scope,
    },
};

#[derive(Debug, Serialize, ToSchema)]
pub struct ListSavedViewsResponse {
    pub views: Vec<SavedViewResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteSavedViewResponse {
    pub deleted: bool,
}

pub fn tables_controller() -> Router<AppState> {
    Router::new()
        .route("/schema", get(get_schema))
        .route("/schema/ts", get(get_ts_schema))
        .route("/", post(create_collection).get(list_collections))
        .route(
            "/{name}",
            get(get_collection)
                .put(update_collection)
                .delete(delete_collection),
        )
        .route("/{name}/$create", get(create_policy))
        // Layouts
        .route("/{name}/layouts", get(list_layouts).post(create_layout))
        .route("/{name}/layout", get(resolve_layout))
        .route(
            "/{name}/layouts/{layout_id}",
            put(update_layout).delete(delete_layout),
        )
        .route(
            "/{name}/layouts/{layout_id}/roles",
            get(get_layout_roles).put(set_layout_roles),
        )
        // Layout-scoped sections
        .route(
            "/{name}/layouts/{layout_id}/sections",
            get(list_sections)
                .post(create_section)
                .patch(reorder_sections),
        )
        .route(
            "/{name}/layouts/{layout_id}/sections/{section_id}",
            put(update_section).delete(delete_section),
        )
        // Saved views
        .route("/{name}/views", get(list_views).post(create_view))
        .route("/{name}/views/{id}", put(update_view).delete(delete_view))
        .route("/{name}/views/{id}/default", put(set_default_view))
}

#[utoipa::path(get, path = "/api/app/collections/schema",
    responses(
        (status = OK, body = Value)
    )
)]
async fn get_schema(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<DatabaseSchema>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let schema = state.database_schema.read().await;

    Ok(Json(success(DatabaseSchema {
        tables: schema.tables.clone(),
        columns: schema.columns.clone(),
        app_versions: schema.app_versions.clone(),
        fields: schema.fields.clone(),
    })))
}

#[utoipa::path(get, path = "/api/app/collections/schema/ts",
    responses(
        (status = OK, body = String)
    )
)]
async fn get_ts_schema(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    headers: HeaderMap,
) -> impl IntoResponse {
    match require_admin(&state, auth_level).await {
        Err(e) => {
            tracing::error!("[TS-SCHEMA] admin check failed: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "text/text")],
                "Admin access required".to_string(),
            );
        }
        Ok(_) => (),
    };
    let version_name = match headers.get("x-version") {
        Some(version_value) => match version_value.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    [(header::CONTENT_TYPE, "text/text")],
                    "x-version header contained invalid UTF-8.".to_string(),
                );
            }
        },
        None => {
            return (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "text/text")],
                "x-version header was not provided.".to_string(),
            );
        }
    };
    let schema = state.database_schema.read().await;

    let mut types = "type alcedocore_apps = {\n".to_string();
    let schemas_from_version: Vec<_> = schema
        .app_versions
        .iter()
        .filter(|av| av.version_name == version_name)
        .collect();

    for version in schemas_from_version {
        types += &format!("\t{}: {{\n", version.app_name);
        for table in schema
            .tables
            .iter()
            .filter(|t| t.schema == version.schema_name)
        {
            let mut table_type = format!("\t\t{}:{{\n", table.name);

            for column in schema
                .columns
                .iter()
                .filter(|col| col.schema == version.schema_name && col.table == table.name)
            {
                let ts_type = if let Some(fk) = &column.foreign_key {
                    let find_app = schema
                        .tables
                        .iter()
                        .find(|t| t.schema == fk.schema && t.name == fk.table)
                        .unwrap();
                    let find_version = schema
                        .app_versions
                        .iter()
                        .find(|av| av.schema_name == fk.schema)
                        .unwrap();

                    format!(
                        "{}|alcedocore_apps[\"{}\"][\"{}\"]",
                        column_type_to_ts(column.data_type.to_string()),
                        find_version.app_name,
                        find_app.name
                    )
                } else {
                    column_type_to_ts(column.data_type.to_string()).to_string()
                };
                let optional = if column.is_nullable { "?" } else { "" };
                table_type.push_str(&format!(
                    "\t\t\t{}{}: {};\n",
                    column.name, optional, ts_type
                ));
            }

            table_type += "\t\t},\n";
            types.push_str(&table_type);
        }
        types += "\t}\n";
    }
    types += "}\n";

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/typescript")],
        types,
    )
}

#[utoipa::path(get, path = "/api/app/collections",
    params(
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK, body = Value)
    )
)]
async fn list_collections(
    State(state): State<AppState>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let result = collections_service::list_collections(&state, &context).await?;
    Ok(Json(success(serde_json::json!({ "collections": result }))))
}

#[utoipa::path(get, path = "/api/app/collections/{name}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK, body = Value)
    )
)]
async fn get_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<CollectionResponse>>, AlcedoError> {
    let result = collections_service::get_collection(&state, &context, &name).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(post, path = "/api/app/collections",
    params(
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = CreateCollectionRequest,
    responses(
        (status = OK, body = Value)
    )
)]
async fn create_collection(
    State(state): State<AppState>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(req): Json<CreateCollectionRequest>,
) -> Result<Json<JSendResponse<CollectionResponse>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result = collections_service::create_collection(&state, &context, req).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(put, path = "/api/app/collections/{name}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = UpdateCollectionRequest,
    responses(
        (status = OK, body = Value)
    )
)]
async fn update_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(req): Json<UpdateCollectionRequest>,
) -> Result<Json<JSendResponse<CollectionResponse>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result = collections_service::update_collection(&state, &context, &name, req).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(delete, path = "/api/app/collections/{name}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK, body = Value)
    )
)]
async fn delete_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.delete").await?;
    collections_service::delete_collection(&state, &context, &name).await?;
    Ok(Json(success(serde_json::json!({ "deleted": true }))))
}

#[utoipa::path(get, path = "/api/app/collections/{name}/$create",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK, body = Value)
    )
)]
async fn create_policy(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let result = collections_service::create_policy(&state, &context, &name).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(get, path = "/api/app/collections/{name}/layouts",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = Value))
)]
async fn list_layouts(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let layouts = collections_service::list_layouts(&state, &context, &name).await?;
    Ok(Json(success(serde_json::json!({ "layouts": layouts }))))
}

#[utoipa::path(post, path = "/api/app/collections/{name}/layouts",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = Value,
    responses((status = OK, body = Value))
)]
async fn create_layout(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(body): Json<Value>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let layout_name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Layout")
        .to_string();
    let result = collections_service::create_layout(&state, &context, &name, &layout_name).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(put, path = "/api/app/collections/{name}/layouts/{layout_id}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = Value,
    responses((status = OK, body = Value))
)]
async fn update_layout(
    State(state): State<AppState>,
    Path((name, layout_id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(body): Json<Value>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result =
        collections_service::update_layout(&state, &context, &name, &layout_id, &body).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(delete, path = "/api/app/collections/{name}/layouts/{layout_id}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = Value))
)]
async fn delete_layout(
    State(state): State<AppState>,
    Path((name, layout_id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result = collections_service::delete_layout(&state, &context, &name, &layout_id).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(get, path = "/api/app/collections/{name}/layout",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = Value))
)]
async fn resolve_layout(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let result = collections_service::resolve_layout(&state, &context, &name).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(get, path = "/api/app/collections/{name}/layouts/{layout_id}/roles",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = Value))
)]
async fn get_layout_roles(
    State(state): State<AppState>,
    Path((name, layout_id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let result = collections_service::get_layout_roles(&state, &context, &name, &layout_id).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(put, path = "/api/app/collections/{name}/layouts/{layout_id}/roles",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = Value,
    responses((status = OK, body = Value))
)]
async fn set_layout_roles(
    State(state): State<AppState>,
    Path((name, layout_id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(body): Json<Value>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result =
        collections_service::set_layout_roles(&state, &context, &name, &layout_id, &body).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(get, path = "/api/app/collections/{name}/layouts/{layout_id}/sections",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = Value))
)]
async fn list_sections(
    State(state): State<AppState>,
    Path((name, layout_id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let sections = collections_service::list_sections(&state, &context, &name, &layout_id).await?;
    Ok(Json(success(serde_json::json!({ "sections": sections }))))
}

#[utoipa::path(post, path = "/api/app/collections/{name}/layouts/{layout_id}/sections",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = SectionRequest,
    responses((status = OK, body = Value))
)]
async fn create_section(
    State(state): State<AppState>,
    Path((name, layout_id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(req): Json<SectionRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result =
        collections_service::create_section(&state, &context, &name, &layout_id, req).await?;
    Ok(Json(success(result)))
}

#[utoipa::path(put, path = "/api/app/collections/{name}/layouts/{layout_id}/sections/{section_id}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("section_id" = String, Path, description = "Section id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = SectionRequest,
    responses((status = OK, body = Value))
)]
async fn update_section(
    State(state): State<AppState>,
    Path((name, layout_id, section_id)): Path<(String, String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(req): Json<SectionRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result =
        collections_service::update_section(&state, &context, &name, &layout_id, &section_id, req)
            .await?;
    Ok(Json(success(result)))
}

#[utoipa::path(delete, path = "/api/app/collections/{name}/layouts/{layout_id}/sections/{section_id}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("section_id" = String, Path, description = "Section id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = Value))
)]
async fn delete_section(
    State(state): State<AppState>,
    Path((name, layout_id, section_id)): Path<(String, String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result =
        collections_service::delete_section(&state, &context, &name, &layout_id, &section_id)
            .await?;
    Ok(Json(success(result)))
}

#[utoipa::path(patch, path = "/api/app/collections/{name}/layouts/{layout_id}/sections",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("layout_id" = String, Path, description = "Layout id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = Value,
    responses((status = OK, body = Value))
)]
async fn reorder_sections(
    State(state): State<AppState>,
    Path((name, layout_id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(body): Json<Value>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let result =
        collections_service::reorder_sections(&state, &context, &name, &layout_id, &body).await?;
    Ok(Json(success(result)))
}

// ---------------------------------------------------------------------------
// Saved views
// ---------------------------------------------------------------------------

#[utoipa::path(get, path = "/api/app/collections/{name}/views",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = JSendResponse<ListSavedViewsResponse>))
)]
async fn list_views(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<ListSavedViewsResponse>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.read").await?;
    let service = SavedViewsService::new(&state, &context);
    let views = service.list_views(&name).await?;
    Ok(Json(success(ListSavedViewsResponse { views })))
}

#[utoipa::path(post, path = "/api/app/collections/{name}/views",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = CreateSavedViewRequest,
    responses((status = OK, body = JSendResponse<SavedViewResponse>))
)]
async fn create_view(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(req): Json<CreateSavedViewRequest>,
) -> Result<Json<JSendResponse<SavedViewResponse>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let service = SavedViewsService::new(&state, &context);
    let view = service.create_view(&name, &req).await?;
    Ok(Json(success(view)))
}

#[utoipa::path(put, path = "/api/app/collections/{name}/views/{id}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("id" = String, Path, description = "Saved view id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body = UpdateSavedViewRequest,
    responses((status = OK, body = JSendResponse<SavedViewResponse>))
)]
async fn update_view(
    State(state): State<AppState>,
    Path((name, id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
    Json(req): Json<UpdateSavedViewRequest>,
) -> Result<Json<JSendResponse<SavedViewResponse>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let view_id = Uuid::parse_str(&id)
        .map_err(|_| AlcedoError::InvalidInput(format!("Invalid view id: {}", id), 0))?;
    let service = SavedViewsService::new(&state, &context);
    let view = service.update_view(&view_id, &req).await?;
    Ok(Json(success(view)))
}

#[utoipa::path(delete, path = "/api/app/collections/{name}/views/{id}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("id" = String, Path, description = "Saved view id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = JSendResponse<DeleteSavedViewResponse>))
)]
async fn delete_view(
    State(state): State<AppState>,
    Path((_name, id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<DeleteSavedViewResponse>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let view_id = Uuid::parse_str(&id)
        .map_err(|_| AlcedoError::InvalidInput(format!("Invalid view id: {}", id), 0))?;
    let service = SavedViewsService::new(&state, &context);
    service.delete_view(&view_id).await?;
    Ok(Json(success(DeleteSavedViewResponse { deleted: true })))
}

#[utoipa::path(put, path = "/api/app/collections/{name}/views/{id}/default",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("id" = String, Path, description = "Saved view id."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses((status = OK, body = JSendResponse<SavedViewResponse>))
)]
async fn set_default_view(
    State(state): State<AppState>,
    Path((_name, id)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<SavedViewResponse>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "collections.write").await?;
    let view_id = Uuid::parse_str(&id)
        .map_err(|_| AlcedoError::InvalidInput(format!("Invalid view id: {}", id), 0))?;
    let service = SavedViewsService::new(&state, &context);
    let view = service.set_default_view(&view_id).await?;
    Ok(Json(success(view)))
}
