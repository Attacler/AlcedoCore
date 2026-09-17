use alcedo_common::RequestIdentity;
use alcedo_db::db::filter_condition::{
    ComparisonOperator, FilterCondition, LogicOperator, SortField,
};
use alcedo_db::services::items::read::{ListRequest, OneRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use alcedo_db::services::items::write::{
    execute_create_for_table, execute_delete_for_table, execute_update_one_for_table,
};
use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check::{extract_user_id_from_session, require_permission};
use crate::error::AppError;
use crate::events::SystemEvent;
use crate::plugins::health::AppState;

#[derive(Deserialize)]
pub struct ListFilesParams {
    pub search: Option<String>,
    pub mime_type: Option<String>,
    pub folder_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Deserialize `folder_id` so missing (None), explicit null (Some(None) →
/// move to root), and a folder UUID string (Some(Some(_))) stay
/// distinguishable. Plain `Option<_>` collapses explicit null into None, so a
/// custom visitor is required.
fn deserialize_optional_folder_id<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct FolderIdVisitor;
    impl<'de> serde::de::Visitor<'de> for FolderIdVisitor {
        type Value = Option<Option<String>>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a folder UUID string, null, or missing")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(None))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(None))
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            Ok(Some(Some(s)))
        }
    }
    deserializer.deserialize_option(FolderIdVisitor)
}

#[derive(Deserialize)]
pub struct UpdateFileMetadataBody {
    pub alt_text: Option<String>,
    pub filename: Option<String>,
    // Double-option so missing (None), explicit null (Some(None) → move to
    // root), and a folder UUID (Some(Some(_))) stay distinguishable. A plain
    // `Option<Value>` would collapse explicit null into None.
    #[serde(default, deserialize_with = "deserialize_optional_folder_id")]
    pub folder_id: Option<Option<String>>,
}

#[derive(Deserialize)]
pub struct CreateFolderBody {
    pub name: String,
    pub parent_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct UpdateFolderBody {
    pub name: Option<String>,
    pub parent_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct DeleteFolderQuery {
    pub recursive: Option<bool>,
}

#[derive(Deserialize)]
pub struct BatchDeleteFilesBody {
    pub ids: Vec<Uuid>,
}

async fn check_file_permission(
    state: &Arc<AppState>,
    identity: &RequestIdentity,
    headers: &axum::http::HeaderMap,
    file_id: &Uuid,
    required_action: &str,
) -> Result<(), AppError> {
    let uid = if let Some(uid) = identity.user_id {
        Some(uid)
    } else {
        extract_user_id_from_session(state, headers).await?
    };
    if let Some(uid) = uid {
        let is_admin: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM alcedocore_user_roles ur JOIN alcedocore_role_scopes rs ON rs.role_id = ur.role_id WHERE ur.user_id = $1 AND rs.scope = 'users.all')"#,
        )
        .bind(uid)
        .fetch_one(&state.db_for_headers(headers).await?)
        .await
        .unwrap_or(false);
        if is_admin {
            return Ok(());
        }
    }

    let link = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT item_id, collection_name, field_name FROM alcedocore_item_files WHERE file_id = $1 LIMIT 1",
    )
    .bind(file_id)
    .fetch_optional(&state.db_for_headers(headers).await?)
    .await?;

    if let Some((_item_id, collection_name, _field_name)) = link {
        require_permission(state, identity, headers, &collection_name, required_action).await?;
    } else {
        return Err(AppError::Forbidden(
            "File is not linked to any collection".to_string(),
        ));
    }

    Ok(())
}

async fn build_folder_path(pool: &sqlx::PgPool, folder_id: Uuid) -> Result<String, AppError> {
    let mut path_parts: Vec<String> = Vec::new();
    let mut current_id = Some(folder_id);

    while let Some(fid) = current_id {
        let row = sqlx::query_as::<_, (String, Option<Uuid>)>(
            "SELECT name, parent_id FROM alcedocore_file_folders WHERE id = $1",
        )
        .bind(fid)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to query folder path: {}", e),
        })?
        .ok_or_else(|| AppError::NotFound("Folder not found".to_string()))?;

        path_parts.push(row.0);
        current_id = row.1;
    }

    path_parts.reverse();
    Ok(path_parts.join("/"))
}

async fn check_file_conflict(
    pool: &sqlx::PgPool,
    filename: &str,
    folder_id: Option<Uuid>,
    excluding_file_id: Option<Uuid>,
) -> Result<bool, AppError> {
    let exists = if let Some(exclude_id) = excluding_file_id {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM alcedocore_file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2 AND id != $3)",
        )
        .bind(filename)
        .bind(folder_id)
        .bind(exclude_id)
        .fetch_one(pool)
        .await
    } else {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM alcedocore_file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2)",
        )
        .bind(filename)
        .bind(folder_id)
        .fetch_one(pool)
        .await
    }
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to check file conflict: {}", e),
    })?;

    Ok(exists)
}

async fn validate_no_circular_ref(
    pool: &sqlx::PgPool,
    folder_id: Uuid,
    new_parent_id: Option<Uuid>,
) -> Result<(), AppError> {
    if let Some(parent) = new_parent_id {
        if parent == folder_id {
            return Err(AppError::BadRequest(
                "A folder cannot be its own parent".to_string(),
            ));
        }
        let mut current = Some(parent);
        while let Some(cid) = current {
            if cid == folder_id {
                return Err(AppError::BadRequest(
                    "Circular folder reference detected".to_string(),
                ));
            }
            current = sqlx::query_scalar::<_, Option<Uuid>>(
                "SELECT parent_id FROM alcedocore_file_folders WHERE id = $1",
            )
            .bind(cid)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to validate circular ref: {}", e),
            })?
            .and_then(|r| r);
        }
    }
    Ok(())
}

pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(Uuid::nil());

    let mut file_data: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut mime_type: Option<String> = None;
    let mut collection_name: Option<String> = None;
    let mut folder_id: Option<Uuid> = None;
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
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::BadRequest(format!("Failed to read file bytes: {}", e))
                })?;
                file_data = Some(bytes.to_vec());
            }
            "collection_name" => {
                collection_name = Some(field.text().await.map_err(|e| {
                    AppError::BadRequest(format!("Failed to read collection_name: {}", e))
                })?);
            }
            "folder_id" => {
                let text = field.text().await.map_err(|e| {
                    AppError::BadRequest(format!("Failed to read folder_id: {}", e))
                })?;
                if !text.is_empty() {
                    folder_id =
                        Some(Uuid::parse_str(&text).map_err(|_| {
                            AppError::BadRequest("Invalid folder_id UUID".to_string())
                        })?);
                }
            }
            "overwrite" => {
                let text = field.text().await.map_err(|e| {
                    AppError::BadRequest(format!("Failed to read overwrite: {}", e))
                })?;
                overwrite = text == "true";
            }
            _ => {}
        }
    }

    if let Some(ref coll) = collection_name {
        require_permission(&state, &identity, &headers, coll, "update").await?;
    }

    let file_data =
        file_data.ok_or_else(|| AppError::BadRequest("No file provided".to_string()))?;
    let filename = filename.unwrap_or_else(|| "unnamed".to_string());
    let mime_type = mime_type.unwrap_or_else(|| "application/octet-stream".to_string());

    let folder_path = match folder_id {
        Some(fid) => {
            let folder_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM alcedocore_file_folders WHERE id = $1)",
            )
            .bind(fid)
            .fetch_one(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to check folder: {}", e),
            })?;

            if !folder_exists {
                return Err(AppError::NotFound("Folder not found".to_string()));
            }

            build_folder_path(db_pool, fid).await?
        }
        None => String::new(),
    };

    if !overwrite {
        let conflict_exists = check_file_conflict(db_pool, &filename, folder_id, None).await?;
        if conflict_exists {
            return Err(AppError::BadRequest(format!(
                "A file named '{}' already exists in the target location. Set overwrite=true to replace.",
                filename
            )));
        }
    } else {
        let existing = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, storage_path FROM alcedocore_file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2",
        )
        .bind(&filename)
        .bind(folder_id)
        .fetch_optional(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to query existing file: {}", e),
        })?;

        if let Some((existing_id, existing_path)) = existing {
            let _ = state.file_storage.delete(&existing_path).await;
            let schema = state.schema_for_headers(&headers).await?;
            let collection = "alcedocore_file_metadata".to_string();
            let engine = ItemsService::for_global(&state.core, &collection);
            let shape = engine
                .privileged_write_shape(
                    TableRef {
                        schema: Some(schema),
                        name: collection.clone(),
                    },
                    &[],
                )
                .await?;
            execute_delete_for_table(db_pool, &shape, vec![serde_json::json!(existing_id.to_string())])
                .await?;
        }
    }

    use sha2::Digest;
    let sha256 = hex::encode(sha2::Sha256::digest(&file_data));

    let storage_path = state
        .file_storage
        .upload(
            bytes::Bytes::from(file_data.clone()),
            &mime_type,
            &filename,
            &folder_path,
        )
        .await
        .map_err(|e| AppError::Internal(format!("File storage upload failed: {}", e)))?;

    let schema = state.schema_for_headers(&headers).await?;
    let collection = "alcedocore_file_metadata".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut meta = serde_json::Map::new();
    meta.insert("filename".to_string(), serde_json::json!(filename));
    meta.insert("mime_type".to_string(), serde_json::json!(mime_type));
    meta.insert(
        "size_bytes".to_string(),
        serde_json::json!(file_data.len() as i64),
    );
    meta.insert("storage_provider".to_string(), serde_json::json!("local"));
    meta.insert(
        "storage_path".to_string(),
        serde_json::json!(storage_path),
    );
    meta.insert("sha256".to_string(), serde_json::json!(sha256));
    meta.insert(
        "uploaded_by".to_string(),
        serde_json::json!(actor_id.to_string()),
    );
    if let Some(fid) = folder_id {
        meta.insert("folder_id".to_string(), serde_json::json!(fid.to_string()));
    }
    let outcome = execute_create_for_table(db_pool, &shape, vec![meta]).await?;
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("File insert returned no row".to_string()))?;

    let file_id: Uuid = row
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AppError::Internal("Invalid file row: missing id".to_string()))?;
    let event_filename = row
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or(filename.as_str())
        .to_string();
    let event_mime = row
        .get("mime_type")
        .and_then(|v| v.as_str())
        .unwrap_or(mime_type.as_str())
        .to_string();
    let event_size = row
        .get("size_bytes")
        .and_then(|v| v.as_i64())
        .unwrap_or(file_data.len() as i64);

    state.event_bus.emit(SystemEvent::FileUploaded {
        file_id,
        filename: event_filename,
        size_bytes: event_size,
        mime_type: event_mime,
    });

    Ok(Json(file_metadata_json(&row, file_id)))
}

pub async fn download_file(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, [(&'static str, String); 2], Vec<u8>), AppError> {
    check_file_permission(&state, &identity, &headers, &id, "read").await?;

    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let collection = "alcedocore_file_metadata".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);

    let row = engine
        .read_one_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: vec!["storage_path".into(), "mime_type".into(), "filename".into()],
                ..Default::default()
            },
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("File '{}' not found", id)))?;

    let storage_path = row
        .get("storage_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("Invalid file row: missing storage_path".to_string()))?;
    let mime_type = row
        .get("mime_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("Invalid file row: missing mime_type".to_string()))?;
    let filename = row
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("Invalid file row: missing filename".to_string()))?;

    let file = state
        .file_storage
        .download(storage_path)
        .await
        .map_err(|e| AppError::Internal(format!("File storage download failed: {}", e)))?
        .ok_or_else(|| AppError::NotFound("File data not found in storage".to_string()))?;

    Ok((
        StatusCode::OK,
        [
            ("Content-Type", mime_type.to_string()),
            (
                "Content-Disposition",
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        file.1.to_vec(),
    ))
}

pub async fn get_file_metadata(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let collection = "alcedocore_file_metadata".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);

    let row = engine
        .read_one_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: file_metadata_fields(),
                ..Default::default()
            },
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("File '{}' not found", id)))?;

    Ok(Json(file_metadata_json(&row, id)))
}

fn file_metadata_fields() -> Vec<String> {
    vec![
        "id".into(),
        "filename".into(),
        "mime_type".into(),
        "size_bytes".into(),
        "storage_provider".into(),
        "storage_path".into(),
        "sha256".into(),
        "alt_text".into(),
        "uploaded_by".into(),
        "folder_id".into(),
        "created_at".into(),
        "updated_at".into(),
    ]
}

fn file_metadata_json(row: &serde_json::Value, file_id: Uuid) -> serde_json::Value {
    let get = |key: &str| row.get(key).cloned().unwrap_or(serde_json::Value::Null);
    json!({
        "id": get("id"),
        "filename": get("filename"),
        "mime_type": get("mime_type"),
        "size_bytes": get("size_bytes"),
        "storage_provider": get("storage_provider"),
        "storage_path": get("storage_path"),
        "sha256": get("sha256"),
        "alt_text": get("alt_text"),
        "uploaded_by": get("uploaded_by"),
        "folder_id": get("folder_id"),
        "created_at": get("created_at"),
        "updated_at": get("updated_at"),
        "download_url": format!("/api/files/{}/download", file_id),
    })
}

pub async fn list_files(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(params): Query<ListFilesParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;

    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    let mut conditions: Vec<FilterCondition> = Vec::new();

    if let Some(ref search) = params.search {
        conditions.push(FilterCondition::Rule {
            field: "filename".into(),
            operator: ComparisonOperator::Ilike,
            value: Some(json!(search)),
        });
    }

    if let Some(ref mime_type) = params.mime_type {
        conditions.push(FilterCondition::Rule {
            field: "mime_type".into(),
            operator: ComparisonOperator::StartsWith,
            value: Some(json!(mime_type)),
        });
    }

    if let Some(ref folder_id) = params.folder_id {
        if folder_id.is_empty() {
            conditions.push(FilterCondition::Rule {
                field: "folder_id".into(),
                operator: ComparisonOperator::IsNull,
                value: None,
            });
        } else {
            let fid = Uuid::parse_str(folder_id)
                .map_err(|_| AppError::BadRequest("Invalid folder_id UUID".to_string()))?;
            conditions.push(FilterCondition::Rule {
                field: "folder_id".into(),
                operator: ComparisonOperator::Eq,
                value: Some(json!(fid.to_string())),
            });
        }
    }

    let filter = if conditions.is_empty() {
        None
    } else {
        Some(FilterCondition::Group {
            operator: LogicOperator::And,
            conditions,
        })
    };

    let collection = "alcedocore_file_metadata".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            ListRequest {
                fields: file_metadata_fields(),
                filter,
                sort: vec![SortField {
                    field: "created_at".into(),
                    order: "desc".into(),
                }],
                limit: limit.max(0) as u64,
                offset: offset.max(0) as u64,
                ..Default::default()
            },
        )
        .await?;

    let data: Vec<serde_json::Value> = result
        .items
        .iter()
        .map(|row| {
            let file_id = row
                .get("id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| AppError::Internal("Invalid file row: missing id".to_string()))?;
            Ok(file_metadata_json(row, file_id))
        })
        .collect::<Result<_, AppError>>()?;
    let total = result.total;

    Ok(Json(json!({ "data": data, "total": total })))
}

pub async fn delete_file(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    check_file_permission(&state, &identity, &headers, &id, "update").await?;

    let row = sqlx::query_as::<_, (String, String)>(
        r#"SELECT storage_path, filename FROM alcedocore_file_metadata WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to query file metadata: {}", e),
    })?
    .ok_or_else(|| AppError::NotFound(format!("File '{}' not found", id)))?;

    state
        .file_storage
        .delete(&row.0)
        .await
        .map_err(|e| AppError::Internal(format!("File storage delete failed: {}", e)))?;

    let schema = state.schema_for_headers(&headers).await?;
    let collection = "alcedocore_file_metadata".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    execute_delete_for_table(db_pool, &shape, vec![serde_json::json!(id.to_string())]).await?;

    state.event_bus.emit(SystemEvent::FileDeleted {
        file_id: id,
        filename: row.1.clone(),
    });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_file_metadata(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateFileMetadataBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let current = sqlx::query_as::<_, (String, Option<Uuid>, String)>(
        "SELECT filename, folder_id, storage_path FROM alcedocore_file_metadata WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to query file: {}", e),
    })?
    .ok_or_else(|| AppError::NotFound(format!("File '{}' not found", id)))?;

    let (current_filename, current_folder_id, current_storage_path) = current;
    let new_filename = body.filename.as_deref().unwrap_or(&current_filename);

    let new_folder_id: Option<Option<Uuid>> = match &body.folder_id {
        Some(None) => Some(None),
        Some(Some(s)) if !s.is_empty() => {
            let fid = Uuid::parse_str(s)
                .map_err(|_| AppError::BadRequest("Invalid folder_id UUID".to_string()))?;
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM alcedocore_file_folders WHERE id = $1)",
            )
            .bind(fid)
            .fetch_one(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to check folder: {}", e),
            })?;
            if !exists {
                return Err(AppError::NotFound("Target folder not found".to_string()));
            }
            Some(Some(fid))
        }
        _ => None,
    };

    let resolved_folder_id = new_folder_id.unwrap_or(current_folder_id);

    let new_storage_path = if new_filename != current_filename || new_folder_id.is_some() {
        let mut actual_path = format!("{}-{}", Uuid::new_v4(), new_filename);

        let conflict_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM alcedocore_file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2 AND id != $3)",
        )
        .bind(new_filename)
        .bind(resolved_folder_id)
        .bind(id)
        .fetch_one(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to check conflict: {}", e),
        })?;

        if conflict_exists {
            return Err(AppError::BadRequest(format!(
                "A file named '{}' already exists in the target location",
                new_filename
            )));
        }

        if let Ok(Some((mime, data))) = state.file_storage.download(&current_storage_path).await {
            let _ = state.file_storage.delete(&current_storage_path).await;
            let folder_path_for_storage = match resolved_folder_id {
                Some(fid) => build_folder_path(db_pool, fid).await?,
                None => String::new(),
            };

            actual_path = state
                .file_storage
                .upload(data, &mime, new_filename, &folder_path_for_storage)
                .await
                .map_err(|e| AppError::Internal(format!("File storage move failed: {}", e)))?;
        }

        actual_path
    } else {
        current_storage_path
    };

    let schema = state.schema_for_headers(&headers).await?;
    let collection = "alcedocore_file_metadata".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut body_map = serde_json::Map::new();
    if let Some(alt_text) = &body.alt_text {
        body_map.insert("alt_text".to_string(), serde_json::json!(alt_text));
    }
    if let Some(filename) = &body.filename {
        body_map.insert("filename".to_string(), serde_json::json!(filename));
    }
    match &body.folder_id {
        // Explicit null must be written as NULL, not omitted — otherwise the
        // storage path (moved to root above) and folder_id diverge (old COALESCE bug).
        Some(None) => {
            body_map.insert("folder_id".to_string(), serde_json::Value::Null);
        }
        Some(Some(s)) if !s.is_empty() => {
            body_map.insert("folder_id".to_string(), serde_json::json!(s));
        }
        _ => {}
    }
    body_map.insert(
        "storage_path".to_string(),
        serde_json::json!(new_storage_path),
    );
    let outcome = match execute_update_one_for_table(
        db_pool,
        &shape,
        &serde_json::json!(id.to_string()),
        &body_map,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(AppError::NotFound(_)) => {
            return Err(AppError::NotFound(format!("File '{}' not found", id)));
        }
        Err(e) => return Err(e),
    };
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("File '{}' not found", id)))?;

    Ok(Json(file_metadata_json(&row, id)))
}

pub async fn batch_delete_files(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<BatchDeleteFilesBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let schema = state.schema_for_headers(&headers).await?;
    let collection = "alcedocore_file_metadata".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;

    let mut deleted: i64 = 0;
    let mut errors: Vec<String> = Vec::new();

    for id in &body.ids {
        let row = match sqlx::query_as::<_, (String,)>(
            "SELECT storage_path FROM alcedocore_file_metadata WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(db_pool)
        .await
        {
            Ok(Some(r)) => r,
            Ok(None) => {
                errors.push(format!("File '{}' not found", id));
                continue;
            }
            Err(e) => {
                errors.push(format!("Error querying file '{}': {}", id, e));
                continue;
            }
        };

        if let Err(e) = state.file_storage.delete(&row.0).await {
            errors.push(format!("Error deleting file '{}' from storage: {}", id, e));
            continue;
        }

        match execute_delete_for_table(db_pool, &shape, vec![serde_json::json!(id.to_string())]).await
        {
            Ok(_) => deleted += 1,
            Err(e) => {
                errors.push(format!("Error deleting file '{}' metadata: {}", id, e));
            }
        }
    }

    Ok(Json(json!({ "deleted": deleted, "errors": errors })))
}

pub async fn create_folder(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateFolderBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    let actor_id = extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(Uuid::nil());

    if let Some(pid) = body.parent_id {
        let parent_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM alcedocore_file_folders WHERE id = $1)",
        )
        .bind(pid)
        .fetch_one(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to check parent folder: {}", e),
        })?;

        if !parent_exists {
            return Err(AppError::NotFound("Parent folder not found".to_string()));
        }
    }

    let sibling_conflict = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM alcedocore_file_folders WHERE name = $1 AND parent_id IS NOT DISTINCT FROM $2)",
    )
    .bind(&body.name)
    .bind(body.parent_id)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to check folder name conflict: {}", e),
    })?;

    if sibling_conflict {
        return Err(AppError::BadRequest(format!(
            "A folder named '{}' already exists in this location",
            body.name
        )));
    }

    let file_conflict = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM alcedocore_file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2)",
    )
    .bind(&body.name)
    .bind(body.parent_id)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to check file name conflict: {}", e),
    })?;

    if file_conflict {
        return Err(AppError::BadRequest(format!(
            "A file named '{}' already exists in this location",
            body.name
        )));
    }

    let schema = state.schema_for_headers(&headers).await?;
    let collection = "alcedocore_file_folders".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut folder_map = serde_json::Map::new();
    folder_map.insert("name".to_string(), serde_json::json!(body.name));
    if let Some(pid) = body.parent_id {
        folder_map.insert("parent_id".to_string(), serde_json::json!(pid.to_string()));
    }
    folder_map.insert(
        "created_by".to_string(),
        serde_json::json!(actor_id.to_string()),
    );
    let outcome = execute_create_for_table(db_pool, &shape, vec![folder_map]).await?;
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("Folder insert returned no row".to_string()))?;

    Ok(Json(folder_json(&row)))
}

pub async fn list_folders(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(params): Query<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let parent_id = params
        .get("parent_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let filter = if let Some(pid) = parent_id {
        let pid = Uuid::parse_str(pid)
            .map_err(|_| AppError::BadRequest("Invalid parent_id UUID".to_string()))?;
        Some(FilterCondition::Rule {
            field: "parent_id".into(),
            operator: ComparisonOperator::Eq,
            value: Some(json!(pid.to_string())),
        })
    } else {
        Some(FilterCondition::Rule {
            field: "parent_id".into(),
            operator: ComparisonOperator::IsNull,
            value: None,
        })
    };

    let collection = "alcedocore_file_folders".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            ListRequest {
                fields: folder_fields(),
                filter,
                sort: vec![SortField {
                    field: "name".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let data: Vec<serde_json::Value> = result
        .items
        .iter()
        .map(|row| folder_json(row))
        .collect();

    Ok(Json(json!({ "data": data })))
}

fn folder_fields() -> Vec<String> {
    vec![
        "id".into(),
        "name".into(),
        "parent_id".into(),
        "created_by".into(),
        "created_at".into(),
        "updated_at".into(),
    ]
}

fn folder_json(row: &serde_json::Value) -> serde_json::Value {
    let get = |key: &str| row.get(key).cloned().unwrap_or(serde_json::Value::Null);
    json!({
        "id": get("id"),
        "name": get("name"),
        "parent_id": get("parent_id"),
        "created_by": get("created_by"),
        "created_at": get("created_at"),
        "updated_at": get("updated_at"),
    })
}

pub async fn get_folder(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let folders_collection = "alcedocore_file_folders".to_string();
    let engine = ItemsService::for_global(&state.core, &folders_collection);

    let folder = engine
        .read_one_for_table(
            &pool,
            TableRef {
                schema: Some(schema.clone()),
                name: folders_collection.clone(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: folder_fields(),
                ..Default::default()
            },
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Folder not found".to_string()))?;

    let path = build_folder_path(&pool, id).await?;

    // limit 0 discards rows; the engine still runs the COUNT, so result.total is the count.
    let subfolder_result = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema.clone()),
                name: folders_collection.clone(),
            },
            ListRequest {
                fields: vec!["id".into()],
                filter: Some(FilterCondition::Rule {
                    field: "parent_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(id.to_string())),
                }),
                limit: 0,
                ..Default::default()
            },
        )
        .await?;
    let subfolder_count = subfolder_result.total;

    let files_collection = "alcedocore_file_metadata".to_string();
    let files_engine = ItemsService::for_global(&state.core, &files_collection);
    let file_result = files_engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: files_collection.clone(),
            },
            ListRequest {
                fields: vec!["id".into()],
                filter: Some(FilterCondition::Rule {
                    field: "folder_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(id.to_string())),
                }),
                limit: 0,
                ..Default::default()
            },
        )
        .await?;
    let file_count = file_result.total;

    let mut body = folder_json(&folder);
    body["path"] = json!(path);
    body["subfolder_count"] = json!(subfolder_count);
    body["file_count"] = json!(file_count);

    Ok(Json(body))
}

pub async fn update_folder(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateFolderBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let current = sqlx::query_as::<_, (String, Option<Uuid>)>(
        "SELECT name, parent_id FROM alcedocore_file_folders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to query folder: {}", e),
    })?
    .ok_or_else(|| AppError::NotFound("Folder not found".to_string()))?;

    let old_name = current.0;
    let old_parent_id = current.1;
    let new_name = body.name.as_deref().unwrap_or(&old_name);
    let new_parent_id = body.parent_id.or(old_parent_id);

    let old_path = build_folder_path(db_pool, id).await?;

    if body.parent_id.is_some() {
        validate_no_circular_ref(db_pool, id, new_parent_id).await?;

        let sibling_conflict = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM alcedocore_file_folders WHERE name = $1 AND parent_id IS NOT DISTINCT FROM $2 AND id != $3)",
        )
        .bind(new_name)
        .bind(new_parent_id)
        .bind(id)
        .fetch_one(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to check sibling conflict: {}", e),
        })?;

        if sibling_conflict {
            return Err(AppError::BadRequest(format!(
                "A folder named '{}' already exists in the target location",
                new_name
            )));
        }
    }

    if body.name.is_some() && body.name.as_deref() != Some(&old_name) {
        let sibling_conflict = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM alcedocore_file_folders WHERE name = $1 AND parent_id IS NOT DISTINCT FROM $2 AND id != $3)",
        )
        .bind(new_name)
        .bind(new_parent_id)
        .bind(id)
        .fetch_one(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to check sibling conflict: {}", e),
        })?;

        if sibling_conflict {
            return Err(AppError::BadRequest(format!(
                "A folder named '{}' already exists in this location",
                new_name
            )));
        }
    }

    let schema = state.schema_for_headers(&headers).await?;
    let collection = "alcedocore_file_folders".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema.clone()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut folder_map = serde_json::Map::new();
    folder_map.insert("name".to_string(), serde_json::json!(new_name));
    // None means no new parent was supplied (absent or explicit null), so the
    // current parent is kept; NULL is only written when already at root.
    match new_parent_id {
        Some(pid) => {
            folder_map.insert("parent_id".to_string(), serde_json::json!(pid.to_string()));
        }
        None => {
            folder_map.insert("parent_id".to_string(), serde_json::Value::Null);
        }
    }
    match execute_update_one_for_table(
        db_pool,
        &shape,
        &serde_json::json!(id.to_string()),
        &folder_map,
    )
    .await
    {
        Ok(_) => {},
        Err(AppError::NotFound(_)) => {
            return Err(AppError::NotFound("Folder not found".to_string()));
        }
        Err(e) => return Err(e),
    }

    let files_collection = "alcedocore_file_metadata".to_string();
    let files_engine = ItemsService::for_global(&state.core, &files_collection);
    let files_shape = files_engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: files_collection.clone(),
            },
            &[],
        )
        .await?;

    let new_path = build_folder_path(db_pool, id).await?;

    if old_path != new_path {
        let mut all_folder_ids = vec![id];
        let mut queue = vec![id];
        while let Some(fid) = queue.pop() {
            let children: Vec<Uuid> =
                sqlx::query_scalar("SELECT id FROM alcedocore_file_folders WHERE parent_id = $1")
                    .bind(fid)
                    .fetch_all(db_pool)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Failed to query folder children: {}", e),
                    })?;

            for child in children {
                all_folder_ids.push(child);
                queue.push(child);
            }
        }

        for &fid in &all_folder_ids {
            let folder_path = build_folder_path(db_pool, fid).await?;

            let files: Vec<(Uuid, String)> =
                sqlx::query_as("SELECT id, filename FROM alcedocore_file_metadata WHERE folder_id = $1")
                    .bind(fid)
                    .fetch_all(db_pool)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Failed to query folder files: {}", e),
                    })?;

            for (file_id, file_name) in files {
                let new_storage_path = format!("{}/{}", folder_path, file_name);

                let old_storage_path: String =
                    sqlx::query_scalar("SELECT storage_path FROM alcedocore_file_metadata WHERE id = $1")
                        .bind(file_id)
                        .fetch_optional(db_pool)
                        .await
                        .map_err(|e| AppError::DatabaseError {
                            details: format!("Failed to query file storage_path: {}", e),
                        })?
                        .unwrap_or_default();

                if let Ok(Some((mime, data))) = state.file_storage.download(&old_storage_path).await
                {
                    let _ = state.file_storage.delete(&old_storage_path).await;
                    let _ = state
                        .file_storage
                        .upload(data, &mime, &file_name, &folder_path)
                        .await;
                }

                let mut file_map = serde_json::Map::new();
                file_map.insert(
                    "storage_path".to_string(),
                    serde_json::json!(new_storage_path),
                );
                execute_update_one_for_table(
                    db_pool,
                    &files_shape,
                    &serde_json::json!(file_id.to_string()),
                    &file_map,
                )
                .await?;
            }
        }
    }

    let updated = sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<Uuid>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, name, parent_id, created_by, created_at, updated_at FROM alcedocore_file_folders WHERE id = $1",
    )
    .bind(id)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to query updated folder: {}", e),
    })?;

    Ok(Json(json!({
        "id": updated.0,
        "name": updated.1,
        "parent_id": updated.2,
        "created_by": updated.3,
        "created_at": updated.4,
        "updated_at": updated.5,
        "path": new_path,
    })))
}

pub async fn delete_folder(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(params): Query<DeleteFolderQuery>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    let recursive = params.recursive.unwrap_or(false);

    let folder_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM alcedocore_file_folders WHERE id = $1)")
            .bind(id)
            .fetch_one(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to check folder: {}", e),
            })?;

    if !folder_exists {
        return Err(AppError::NotFound("Folder not found".to_string()));
    }

    let mut all_folder_ids = vec![id];
    let mut queue = vec![id];
    while let Some(fid) = queue.pop() {
        let children: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM alcedocore_file_folders WHERE parent_id = $1")
                .bind(fid)
                .fetch_all(db_pool)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to query children: {}", e),
                })?;

        for child in children {
            all_folder_ids.push(child);
            queue.push(child);
        }
    }

    let file_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM alcedocore_file_metadata WHERE folder_id = ANY($1)")
            .bind(&all_folder_ids)
            .fetch_one(db_pool)
            .await
            .unwrap_or(0);

    if file_count > 0 && !recursive {
        return Err(AppError::BadRequest(format!(
            "Folder is not empty ({} files). Use recursive=true to delete.",
            file_count
        )));
    }

    let schema = state.schema_for_headers(&headers).await?;
    let folders_collection = "alcedocore_file_folders".to_string();
    let folders_engine = ItemsService::for_global(&state.core, &folders_collection);
    let folders_shape = folders_engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema.clone()),
                name: folders_collection.clone(),
            },
            &[],
        )
        .await?;
    let files_collection = "alcedocore_file_metadata".to_string();
    let files_engine = ItemsService::for_global(&state.core, &files_collection);
    let files_shape = files_engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: files_collection.clone(),
            },
            &[],
        )
        .await?;

    if recursive {
        for &fid in &all_folder_ids {
            let files: Vec<(Uuid, String)> =
                sqlx::query_as("SELECT id, storage_path FROM alcedocore_file_metadata WHERE folder_id = $1")
                    .bind(fid)
                    .fetch_all(db_pool)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Failed to query files in folder: {}", e),
                    })?;

            for (file_id, storage_path) in files {
                let _ = state.file_storage.delete(&storage_path).await;
                let _ = execute_delete_for_table(
                    db_pool,
                    &files_shape,
                    vec![serde_json::json!(file_id.to_string())],
                )
                .await;
            }
        }

        for &fid in all_folder_ids.iter().rev() {
            execute_delete_for_table(
                db_pool,
                &folders_shape,
                vec![serde_json::json!(fid.to_string())],
            )
            .await?;
        }
    }

    if !recursive {
        execute_delete_for_table(db_pool, &folders_shape, vec![serde_json::json!(id.to_string())])
            .await?;
    }

    Ok(StatusCode::NO_CONTENT)
}
