use axum::{
    Json, Router,
    extract::{Multipart, Path, Query, State},
    http::{StatusCode, header},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        context::{AppContext, ExtractContext},
        errors::AlcedoError,
        files::{FilesService, ListFilesParams},
        respond::{JSendResponse, success},
        scopes::require_scope,
    },
};

pub fn files_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_files))
        .route("/upload", post(upload_file))
        .route("/batch/delete", post(batch_delete_files))
        .route("/folders", get(list_folders).post(create_folder))
        .route(
            "/folders/{id}",
            get(get_folder).patch(update_folder).delete(delete_folder),
        )
        .route(
            "/{id}",
            get(get_file).patch(update_file).delete(delete_file),
        )
        .route("/{id}/download", get(download_file))
}

/// Parses a `folder_id`/`parent_id` body field so missing, explicit `null`
/// (move to root) and a UUID string stay distinguishable.
fn deserialize_double_option_uuid<'de, D>(
    deserializer: D,
) -> Result<Option<Option<Uuid>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    match raw {
        None => Ok(Some(None)),
        Some(s) if s.is_empty() => Ok(Some(None)),
        Some(s) => Uuid::parse_str(&s)
            .map(|id| Some(Some(id)))
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateFileRequest {
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option_uuid")]
    pub folder_id: Option<Option<Uuid>>,
    #[serde(default)]
    pub alt_text: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateFolderRequest {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateFolderRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ListFoldersQuery {
    #[serde(default)]
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteFolderQuery {
    #[serde(default)]
    pub recursive: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BatchDeleteFilesBody {
    pub ids: Vec<Uuid>,
}

async fn read_scope(
    state: &AppState,
    auth: &AuthLevel,
    ctx: &AppContext,
) -> Result<(), AlcedoError> {
    require_scope(state, auth, ctx, "items.read").await
}

async fn write_scope(
    state: &AppState,
    auth: &AuthLevel,
    ctx: &AppContext,
) -> Result<(), AlcedoError> {
    require_scope(state, auth, ctx, "items.write").await
}

#[utoipa::path(get, path = "/api/app/files", tag = "Files",
    responses((status = OK, body = Value))
)]
async fn list_files(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Query(params): Query<ListFilesParams>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    read_scope(&state, &auth, &context).await?;
    let data = FilesService::new(&state, &context).list_files(&params).await?;
    Ok(Json(success(data)))
}

#[utoipa::path(post, path = "/api/app/files/upload", tag = "Files",
    responses((status = OK, body = Value))
)]
async fn upload_file(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    mut multipart: Multipart,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let mut file_data: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut mime_type: Option<String> = None;
    let mut folder_id: Option<Uuid> = None;
    let mut collection_name: Option<String> = None;
    let mut overwrite = false;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                mime_type = Some(
                    field
                        .content_type()
                        .unwrap_or("application/octet-stream")
                        .to_string(),
                );
                filename = Some(field.file_name().unwrap_or("unnamed").to_string());
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AlcedoError::InvalidInput(format!("Bad upload: {}", e), 0))?;
                file_data = Some(bytes.to_vec());
            }
            "folder_id" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| AlcedoError::InvalidInput(format!("Bad folder_id: {}", e), 0))?;
                if !text.is_empty() {
                    folder_id = Some(Uuid::parse_str(&text).map_err(|_| {
                        AlcedoError::InvalidInput("Invalid folder_id UUID".to_string(), 0)
                    })?);
                }
            }
            "collection_name" => {
                collection_name = Some(field.text().await.unwrap_or_default());
            }
            "overwrite" => {
                let text = field.text().await.unwrap_or_default();
                overwrite = text == "true";
            }
            _ => {}
        }
    }

    FilesService::new(&state, &context)
        .check_upload_permission(collection_name.as_deref())
        .await?;

    let data = file_data.ok_or_else(|| AlcedoError::InvalidInput("No file provided".to_string(), 0))?;
    let filename = filename.unwrap_or_else(|| "unnamed".to_string());
    let mime_type = mime_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let uploaded_by = match auth {
        AuthLevel::User(id) => Some(id),
        _ => None,
    };

    let result = FilesService::new(&state, &context)
        .upload(data, filename, mime_type, folder_id, overwrite, uploaded_by)
        .await?;
    Ok(Json(success(result)))
}

#[utoipa::path(get, path = "/api/app/files/{id}", tag = "Files",
    responses((status = OK, body = Value))
)]
async fn get_file(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<Uuid>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    read_scope(&state, &auth, &context).await?;
    Ok(Json(success(
        FilesService::new(&state, &context).get_file(id).await?,
    )))
}

#[utoipa::path(patch, path = "/api/app/files/{id}", tag = "Files",
    request_body = UpdateFileRequest,
    responses((status = OK, body = Value))
)]
async fn update_file(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateFileRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    write_scope(&state, &auth, &context).await?;
    let alt_text = body.alt_text.map(Some);
    let result = FilesService::new(&state, &context)
        .update_file(id, body.filename, body.folder_id, alt_text)
        .await?;
    Ok(Json(success(result)))
}

#[utoipa::path(delete, path = "/api/app/files/{id}", tag = "Files",
    responses((status = NO_CONTENT))
)]
async fn delete_file(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AlcedoError> {
    write_scope(&state, &auth, &context).await?;
    FilesService::new(&state, &context).delete_file(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/app/files/batch/delete", tag = "Files",
    request_body = BatchDeleteFilesBody,
    responses((status = OK, body = Value))
)]
async fn batch_delete_files(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(body): Json<BatchDeleteFilesBody>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    write_scope(&state, &auth, &context).await?;
    let result = FilesService::new(&state, &context)
        .batch_delete_files(body.ids)
        .await?;
    Ok(Json(success(result)))
}

#[utoipa::path(get, path = "/api/app/files/{id}/download", tag = "Files",
    responses((status = OK, body = Vec<u8>))
)]
async fn download_file(
    State(state): State<AppState>,
    ExtractContext(context): ExtractContext,
    Path(id): Path<Uuid>,
) -> Result<([(header::HeaderName, String); 1], Vec<u8>), AlcedoError> {
    let service = FilesService::new(&state, &context);
    service.check_download_permission(id).await?;
    let (mime, bytes) = service.download(id).await?;
    Ok(([(header::CONTENT_TYPE, mime)], bytes.to_vec()))
}

#[utoipa::path(get, path = "/api/app/files/folders", tag = "Files",
    responses((status = OK, body = Value))
)]
async fn list_folders(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Query(params): Query<ListFoldersQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    read_scope(&state, &auth, &context).await?;
    let data = FilesService::new(&state, &context)
        .list_folders(params.parent_id)
        .await?;
    Ok(Json(success(data)))
}

#[utoipa::path(post, path = "/api/app/files/folders", tag = "Files",
    request_body = CreateFolderRequest,
    responses((status = OK, body = Value))
)]
async fn create_folder(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(body): Json<CreateFolderRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    write_scope(&state, &auth, &context).await?;
    let created_by = match auth {
        AuthLevel::User(id) => Some(id),
        _ => None,
    };
    let data = FilesService::new(&state, &context)
        .create_folder(&body.name, body.parent_id, created_by)
        .await?;
    Ok(Json(success(data)))
}

#[utoipa::path(get, path = "/api/app/files/folders/{id}", tag = "Files",
    responses((status = OK, body = Value))
)]
async fn get_folder(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<Uuid>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    read_scope(&state, &auth, &context).await?;
    Ok(Json(success(
        FilesService::new(&state, &context).get_folder(id).await?,
    )))
}

#[utoipa::path(patch, path = "/api/app/files/folders/{id}", tag = "Files",
    request_body = UpdateFolderRequest,
    responses((status = OK, body = Value))
)]
async fn update_folder(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateFolderRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    write_scope(&state, &auth, &context).await?;
    let parent_id = body.parent_id.map(Some);
    let data = FilesService::new(&state, &context)
        .update_folder(id, body.name, parent_id)
        .await?;
    Ok(Json(success(data)))
}

#[utoipa::path(delete, path = "/api/app/files/folders/{id}", tag = "Files",
    responses((status = NO_CONTENT))
)]
async fn delete_folder(
    State(state): State<AppState>,
    auth: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<Uuid>,
    Query(params): Query<DeleteFolderQuery>,
) -> Result<StatusCode, AlcedoError> {
    write_scope(&state, &auth, &context).await?;
    FilesService::new(&state, &context)
        .delete_folder(id, params.recursive.unwrap_or(false))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
