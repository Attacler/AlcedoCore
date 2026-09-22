use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, post},
};
use sea_query::{Alias, ColumnDef};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    AppState,
    services::{
        self,
        context::ExtractContext,
        errors::AlcedoError,
        postgres::{
            columntypetots::column_type_to_ts,
            inspector::{DatabaseSchema, TableMeta},
            tables::{
                FieldCreationObject, FieldMetaObject, FieldSavedMetaObject, FieldUpdateObject,
            },
        },
        respond::{self, JSendResponse, success},
    },
};

pub fn tables_controller() -> Router<AppState> {
    return Router::new()
        .route("/schema", get(get_schema))
        .route("/schema/ts", get(get_ts_schema))
        .route("/", post(create_collection).get(get_collections))
        .route("/{name}", delete(drop_collection))
        .route("/{name}/fields", post(add_field))
        .route(
            "/{name}/fields/{field}",
            delete(drop_field).put(update_field),
        );
}

#[utoipa::path(get, path = "/api/app/collections/schema", 
    responses(
        (status = OK, body = Value)
    )
)]
async fn get_schema(
    State(state): State<AppState>,
) -> (StatusCode, Json<JSendResponse<DatabaseSchema>>) {
    let schema = state.database_schema.read().await;

    (
        StatusCode::OK,
        Json(success(DatabaseSchema {
            tables: schema.tables.clone(),
            columns: schema.columns.clone(),
            app_versions: schema.app_versions.clone(),
        })),
    )
}

#[utoipa::path(get, path = "/api/app/collections/schema/ts", 
    responses(
        (status = OK, body = String)
    )
)]
async fn get_ts_schema(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let version_name = match headers.get("x-version") {
        Some(version_value) => {
            // Convert HeaderValue to String or &str
            match version_value.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        [(header::CONTENT_TYPE, "text/text")],
                        "x-version header contained invalid UTF-8.".to_string(),
                    );
                }
            }
        }
        None => {
            return (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "text/text")],
                "x-version header was not provided.".to_string(),
            );
        }
    };
    let schema = state.database_schema.read().await;

    let mut types = "type alcedo_apps = {\n".to_string();
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
                        "{}|alcedo_apps[\"{}\"][\"{}\"]",
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

#[derive(ToSchema, Serialize)]
enum CollectionFieldType {
    Integer,
    String,
    Float,
    Boolean,
    Date,
    DateTime,
}

#[derive(ToSchema, Serialize)]
struct GetCollectionFields {
    name: String,
    data_type: CollectionFieldType,
    foreign_key: Option<services::postgres::inspector::ForeignKey>,
    is_unique: bool,
    default_value: Option<String>,
    max_length: Option<i32>,
    numeric_precision: Option<i32>,
    numeric_scale: Option<i32>,
    is_nullable: bool,
    has_auto_increment: bool,
    is_primary_key: bool,
    meta: Option<FieldSavedMetaObject>,
}

#[derive(ToSchema, Serialize)]
struct GetCollectionsResponse {
    name: String,
    app_id: i32,
    version_id: i32,
    meta: Option<TableMeta>,
    fields: Vec<GetCollectionFields>,
}

#[utoipa::path(get, path = "/api/app/collections",
    params(
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK, body = GetCollectionsResponse)
    )
)]
async fn get_collections(
    State(state): State<AppState>,
) -> Result<Json<JSendResponse<Vec<GetCollectionsResponse>>>, AlcedoError> {
    let schema = state.database_schema.read().await;

    let collections = schema
        .tables
        .iter()
        .filter(|t| t.schema.contains("010"))
        .map(|table| {
            let find_version = schema
                .app_versions
                .iter()
                .find(|v| v.schema_name == table.schema)
                .unwrap();

            GetCollectionsResponse {
                name: table.name.clone(),
                app_id: find_version.app_id,
                version_id: find_version.version_id,
                meta: table.meta.clone(),
                fields: schema
                    .columns
                    .iter()
                    .filter(|field| table.name == field.table && field.schema == table.schema)
                    .map(|field| {
                        let data_type = match field.data_type.as_str() {
                            "numeric" => CollectionFieldType::Float,
                            "integer" => CollectionFieldType::Integer,
                            "boolean" => CollectionFieldType::Boolean,
                            "date" => CollectionFieldType::Date,
                            "timestamp"
                            | "timestamp without time zone"
                            | "timestamp with time zone" => CollectionFieldType::DateTime,
                            _ => CollectionFieldType::String,
                        };

                        GetCollectionFields {
                            data_type,
                            name: field.name.clone(),
                            foreign_key: field.foreign_key.clone(),
                            default_value: field.default_value.clone(),
                            has_auto_increment: field.has_auto_increment.clone(),
                            is_nullable: field.is_nullable.clone(),
                            is_unique: field.is_unique.clone(),
                            max_length: field.max_length.clone(),
                            numeric_precision: field.numeric_precision.clone(),
                            numeric_scale: field.numeric_scale.clone(),
                            is_primary_key: field.is_primary_key,
                            meta: field.meta.clone(),
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    Ok(Json(success(collections)))
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct CreateTableMeta {
    pub name: String,
    pub icon_name: Option<String>,
    pub icon_color: Option<String>,
    pub singleton: bool,
    pub hidden: bool,
    pub sort_field: Option<String>,
}

#[derive(Deserialize, ToSchema, Clone)]
struct CreateTableRequest {
    name: String,
    pk: ColumnRequest,
    meta: CreateTableMeta,
}

#[derive(Deserialize, ToSchema, Debug, Clone)]
struct ColumnRequest {
    name: String,
    #[serde(rename = "type")]
    col_type: String,
    #[serde(default)]
    has_auto_increment: bool,
}

#[utoipa::path(post, path = "/api/app/collections",
    params(
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body=CreateTableRequest,
    responses(
        (status = OK, body = JSendResponse<String>)
    )
)]
async fn create_collection(
    State(state): State<AppState>,
    ExtractContext(context): ExtractContext,
    Json(req): Json<CreateTableRequest>,
) -> Result<Json<JSendResponse<String>>, AlcedoError> {
    let manager = services::postgres::tables::TableService::new(&state, &context);

    let meta = TableMeta {
        app_name: context.app_api_name(),
        app_version: context.version_api_name(),
        hidden: req.meta.hidden,
        icon_color: req.meta.icon_color,
        icon_name: req.meta.icon_name,
        name: req.meta.name,
        singleton: req.meta.singleton,
        sort_field: req.meta.sort_field,
        table: req.name.clone(),
        id: None,
    };

    manager
        .create_table(
            &req.name,
            |t| {
                let mut col = into_col(&req.pk.name, &req.pk.col_type);

                col.primary_key();
                if req.pk.has_auto_increment {
                    col.auto_increment();
                }
                t.col(col);
            },
            Some(meta),
            &mut None,
        )
        .await?;
    Ok(Json(respond::success::<String>(
        "Table created".to_string(),
    )))
}

#[utoipa::path(delete, path = "/api/app/collections/{name}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK, body = JSendResponse<String>)
    )
)]
async fn drop_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<String>>, AlcedoError> {
    let manager = services::postgres::tables::TableService::new(&state, &context);

    manager.drop_table(&name, &mut None).await?;
    Ok(Json(respond::success::<String>(
        "Table dropped".to_string(),
    )))
}

#[derive(Deserialize, ToSchema)]
struct AddFieldRequest {
    field: FieldCreationObject,
    meta: FieldMetaObject,
}

#[utoipa::path(post, path = "/api/app/collections/{name}/fields",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body=AddFieldRequest,
    responses(
        (status = OK, body = JSendResponse<String>)
    )
)]
async fn add_field(
    State(state): State<AppState>,
    Path(collection): Path<String>,
    ExtractContext(context): ExtractContext,
    Json(req): Json<AddFieldRequest>,
) -> Result<Json<JSendResponse<String>>, AlcedoError> {
    let manager = services::postgres::tables::TableService::new(&state, &context);

    manager
        .add_field(&collection, req.field, Some(req.meta), &mut None)
        .await?;
    Ok(Json(respond::success::<String>("Field added".to_string())))
}

#[utoipa::path(delete, path = "/api/app/collections/{name}/fields/{field_name}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("field_name" = String, Path, description = "Field name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK, body = JSendResponse<String>)
    )
)]
async fn drop_field(
    State(state): State<AppState>,
    Path((table, field)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<String>>, AlcedoError> {
    let manager = services::postgres::tables::TableService::new(&state, &context);

    manager.drop_field(&table, &field, &mut None).await?;

    Ok(Json(respond::success::<String>(
        "Field dropped".to_string(),
    )))
}

#[derive(Deserialize, ToSchema)]
struct UpdateFieldRequest {
    schema: FieldUpdateObject,
    meta: Option<FieldMetaObject>,
}

#[utoipa::path(put, path = "/api/app/collections/{name}/fields/{field}",
    params(
        ("name" = String, Path, description = "Collection name."),
        ("field" = String, Path, description = "Field name."),
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    request_body=UpdateFieldRequest,
    responses(
        (status = OK, body = JSendResponse<String>)
    )
)]
async fn update_field(
    State(state): State<AppState>,
    Path((table, field)): Path<(String, String)>,
    ExtractContext(context): ExtractContext,
    Json(req): Json<UpdateFieldRequest>,
) -> Result<Json<JSendResponse<String>>, AlcedoError> {
    let manager = services::postgres::tables::TableService::new(&state, &context);

    manager
        .update_field(&table, &field, req.schema, req.meta, &mut None)
        .await?;

    Ok(Json(respond::success::<String>(
        "Field updated".to_string(),
    )))
}

fn into_col(name: &str, t: &str) -> ColumnDef {
    let mut column = ColumnDef::new(Alias::new(name));

    match t.to_lowercase().as_str() {
        "int" | "integer" => column.integer().clone(),
        "text" | "string" => column.string().clone(),
        "bool" | "boolean" => column.boolean().clone(),
        "float" => column.float().clone(),
        _ => column.string().clone(),
    }
}
