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

#[derive(Deserialize)]
pub struct UpdateFileMetadataBody {
    pub alt_text: Option<String>,
    pub filename: Option<String>,
    pub folder_id: Option<serde_json::Value>,
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
    headers: &axum::http::HeaderMap,
    file_id: &Uuid,
    required_action: &str,
) -> Result<(), AppError> {
    let uid = extract_user_id_from_session(state, headers).await?;
    if let Some(uid) = uid {
        let is_admin: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN role_scopes rs ON rs.role_id = ur.role_id WHERE ur.user_id = $1 AND rs.scope = 'users.all')"#,
        )
        .bind(uid)
        .fetch_one(state.db()?)
        .await
        .unwrap_or(false);
        if is_admin {
            return Ok(());
        }
    }

    let link = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT item_id, collection_name, field_name FROM item_files WHERE file_id = $1 LIMIT 1",
    )
    .bind(file_id)
    .fetch_optional(state.db()?)
    .await?;

    if let Some((_item_id, collection_name, _field_name)) = link {
        require_permission(state, headers, &collection_name, required_action).await?;
    } else {
        return Err(AppError::Forbidden("File is not linked to any collection".to_string()));
    }

    Ok(())
}

async fn build_folder_path(pool: &sqlx::PgPool, folder_id: Uuid) -> Result<String, AppError> {
    let mut path_parts: Vec<String> = Vec::new();
    let mut current_id = Some(folder_id);

    while let Some(fid) = current_id {
        let row = sqlx::query_as::<_, (String, Option<Uuid>)>(
            "SELECT name, parent_id FROM file_folders WHERE id = $1",
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
            "SELECT EXISTS(SELECT 1 FROM file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2 AND id != $3)",
        )
        .bind(filename)
        .bind(folder_id)
        .bind(exclude_id)
        .fetch_one(pool)
        .await
    } else {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2)",
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
            return Err(AppError::BadRequest("A folder cannot be its own parent".to_string()));
        }
        let mut current = Some(parent);
        while let Some(cid) = current {
            if cid == folder_id {
                return Err(AppError::BadRequest("Circular folder reference detected".to_string()));
            }
            current = sqlx::query_scalar::<_, Option<Uuid>>(
                "SELECT parent_id FROM file_folders WHERE id = $1",
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
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

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
                filename = Some(
                    field
                        .file_name()
                        .unwrap_or("unnamed")
                        .to_string(),
                );
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
                    folder_id = Some(Uuid::parse_str(&text).map_err(|_| {
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
        require_permission(&state, &headers, coll, "update").await?;
    }

    let file_data =
        file_data.ok_or_else(|| AppError::BadRequest("No file provided".to_string()))?;
    let filename = filename.unwrap_or_else(|| "unnamed".to_string());
    let mime_type = mime_type.unwrap_or_else(|| "application/octet-stream".to_string());

    let folder_path = if let Some(fid) = folder_id {
        let folder_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM file_folders WHERE id = $1)",
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

        let path = build_folder_path(db_pool, fid).await?;
        Some(path)
    } else {
        None
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
            "SELECT id, storage_path FROM file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2",
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
            sqlx::query("DELETE FROM file_metadata WHERE id = $1")
                .bind(existing_id)
                .execute(db_pool)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to delete existing file: {}", e),
                })?;
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
            folder_path.as_deref(),
        )
        .await
        .map_err(|e| AppError::Internal(format!("File storage upload failed: {}", e)))?;

    let row = sqlx::query_as::<_, (
        Uuid, String, String, i64, String, String, Option<String>, Option<String>, Option<Uuid>, Option<Uuid>,
        chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        r#"INSERT INTO file_metadata (filename, mime_type, size_bytes, storage_provider, storage_path, sha256, uploaded_by, folder_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           RETURNING id, filename, mime_type, size_bytes, storage_provider, storage_path, sha256, alt_text, uploaded_by, folder_id, created_at, updated_at"#
    )
    .bind(&filename)
    .bind(&mime_type)
    .bind(file_data.len() as i64)
    .bind("local")
    .bind(&storage_path)
    .bind(&sha256)
    .bind(actor_id)
    .bind(folder_id)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to insert file metadata: {}", e),
    })?;

    state.event_bus.emit(SystemEvent::FileUploaded {
        file_id: row.0,
        filename: row.1.clone(),
        size_bytes: row.3,
        mime_type: row.2.clone(),
    });

    let download_url = format!("/api/files/{}/download", row.0);

    Ok(Json(json!({
        "id": row.0,
        "filename": row.1,
        "mime_type": row.2,
        "size_bytes": row.3,
        "storage_provider": row.4,
        "storage_path": row.5,
        "sha256": row.6,
        "alt_text": row.7,
        "uploaded_by": row.8,
        "folder_id": row.9,
        "created_at": row.10,
        "updated_at": row.11,
        "download_url": download_url,
    })))
}

pub async fn download_file(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<
    (
        StatusCode,
        [(&'static str, String); 2],
        Vec<u8>,
    ),
    AppError,
> {
    let db_pool = state.db()?;

    check_file_permission(&state, &headers, &id, "read").await?;

    let row = sqlx::query_as::<_, (String, String, String)>(
        r#"SELECT storage_path, mime_type, filename FROM file_metadata WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to query file metadata: {}", e),
    })?
    .ok_or_else(|| AppError::NotFound(format!("File '{}' not found", id)))?;

    let (storage_path, mime_type, filename) = row;

    let file = state
        .file_storage
        .download(&storage_path)
        .await
        .map_err(|e| AppError::Internal(format!("File storage download failed: {}", e)))?
        .ok_or_else(|| AppError::NotFound("File data not found in storage".to_string()))?;

    Ok((
        StatusCode::OK,
        [
            ("Content-Type", mime_type.clone()),
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
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    let row = sqlx::query_as::<_, (
        Uuid, String, String, i64, String, String, Option<String>, Option<String>, Option<Uuid>, Option<Uuid>,
        chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        r#"SELECT id, filename, mime_type, size_bytes, storage_provider, storage_path, sha256, alt_text, uploaded_by, folder_id, created_at, updated_at
           FROM file_metadata WHERE id = $1"#
    )
    .bind(id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to query file metadata: {}", e),
    })?
    .ok_or_else(|| AppError::NotFound(format!("File '{}' not found", id)))?;

    Ok(Json(json!({
        "id": row.0,
        "filename": row.1,
        "mime_type": row.2,
        "size_bytes": row.3,
        "storage_provider": row.4,
        "storage_path": row.5,
        "sha256": row.6,
        "alt_text": row.7,
        "uploaded_by": row.8,
        "folder_id": row.9,
        "created_at": row.10,
        "updated_at": row.11,
        "download_url": format!("/api/files/{}/download", row.0),
    })))
}

pub async fn list_files(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListFilesParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    let mut where_conditions: Vec<String> = Vec::new();
    let mut bind_values: Vec<serde_json::Value> = Vec::new();
    let mut bind_idx = 1u32;

    if let Some(ref search) = params.search {
        where_conditions.push(format!("fm.filename ILIKE ${}", bind_idx));
        bind_values.push(serde_json::Value::String(format!("%{}%", search)));
        bind_idx += 1;
    }

    if let Some(ref mime_type) = params.mime_type {
        where_conditions.push(format!("fm.mime_type LIKE ${}", bind_idx));
        bind_values.push(serde_json::Value::String(format!("{}%", mime_type)));
        bind_idx += 1;
    }

    if let Some(ref folder_id) = params.folder_id {
        if folder_id.is_empty() {
            where_conditions.push("fm.folder_id IS NULL".to_string());
        } else {
            let fid = Uuid::parse_str(folder_id).map_err(|_| {
                AppError::BadRequest("Invalid folder_id UUID".to_string())
            })?;
            where_conditions.push(format!("fm.folder_id = ${}::uuid", bind_idx));
            bind_values.push(serde_json::Value::String(fid.to_string()));
            bind_idx += 1;
        }
    }

    let where_clause = if where_conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM file_metadata fm {}", where_clause);
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values {
        count_q = crate::bind_json_value!(count_q, val);
    }
    let total = count_q
        .fetch_one(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to count file metadata: {}", e),
        })?;

    let data_sql = format!(
        r#"SELECT fm.id, fm.filename, fm.mime_type, fm.size_bytes, fm.storage_provider, fm.storage_path, fm.sha256, fm.alt_text, fm.uploaded_by, fm.folder_id, fm.created_at, fm.updated_at
           FROM file_metadata fm {}
           ORDER BY fm.created_at DESC
           LIMIT ${} OFFSET ${}"#,
        where_clause,
        bind_idx,
        bind_idx + 1,
    );
    bind_values.push(serde_json::Value::Number(serde_json::Number::from(limit)));
    bind_values.push(serde_json::Value::Number(serde_json::Number::from(offset)));

    let mut data_q = sqlx::query_as::<_, (
        Uuid, String, String, i64, String, String, Option<String>, Option<String>, Option<Uuid>, Option<Uuid>,
        chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(&data_sql);
    for val in &bind_values {
        data_q = crate::bind_json_value!(data_q, val);
    }
    let rows = data_q
        .fetch_all(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to list file metadata: {}", e),
        })?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.0,
                "filename": row.1,
                "mime_type": row.2,
                "size_bytes": row.3,
                "storage_provider": row.4,
                "storage_path": row.5,
                "sha256": row.6,
                "alt_text": row.7,
                "uploaded_by": row.8,
                "folder_id": row.9,
                "created_at": row.10,
                "updated_at": row.11,
                "download_url": format!("/api/files/{}/download", row.0),
            })
        })
        .collect();

    Ok(Json(json!({ "data": data, "total": total })))
}

pub async fn delete_file(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let db_pool = state.db()?;

    check_file_permission(&state, &headers, &id, "update").await?;

    let row = sqlx::query_as::<_, (String, String)>(
        r#"SELECT storage_path, filename FROM file_metadata WHERE id = $1"#,
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

    sqlx::query("DELETE FROM file_metadata WHERE id = $1")
        .bind(id)
        .execute(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to delete file metadata: {}", e),
        })?;

    state.event_bus.emit(SystemEvent::FileDeleted {
        file_id: id,
        filename: row.1.clone(),
    });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_file_metadata(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateFileMetadataBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    let current = sqlx::query_as::<_, (String, Option<Uuid>, String)>(
        "SELECT filename, folder_id, storage_path FROM file_metadata WHERE id = $1",
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

    let new_folder_id: Option<Option<Uuid>> = match body.folder_id {
        Some(serde_json::Value::Null) => Some(None),
        Some(serde_json::Value::String(ref s)) if !s.is_empty() => {
            let fid = Uuid::parse_str(s).map_err(|_| AppError::BadRequest("Invalid folder_id UUID".to_string()))?;
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM file_folders WHERE id = $1)",
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
        let new_path = if let Some(fid) = resolved_folder_id {
            let folder_path = build_folder_path(db_pool, fid).await?;
            format!("{}/{}", folder_path, new_filename)
        } else {
            format!("{}-{}", Uuid::new_v4(), new_filename)
        };

        let conflict_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2 AND id != $3)",
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
                "A file named '{}' already exists in the target location", new_filename
            )));
        }

        if let Ok(Some((mime, data))) = state.file_storage.download(&current_storage_path).await {
            let _ = state.file_storage.delete(&current_storage_path).await;
            let folder_path_for_storage = if let Some(fid) = resolved_folder_id {
                Some(build_folder_path(db_pool, fid).await?)
            } else {
                None
            };
            state.file_storage.upload(
                data,
                &mime,
                new_filename,
                folder_path_for_storage.as_deref(),
            ).await.map_err(|e| AppError::Internal(format!("File storage move failed: {}", e)))?;
        }

        new_path
    } else {
        current_storage_path
    };

    let row = sqlx::query_as::<_, (
        Uuid, String, String, i64, String, String, Option<String>, Option<String>, Option<Uuid>, Option<Uuid>,
        chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        r#"UPDATE file_metadata
           SET alt_text = COALESCE($1, alt_text),
               filename = COALESCE($2, filename),
               folder_id = COALESCE($3, folder_id),
               storage_path = $4,
               updated_at = NOW()
           WHERE id = $5
           RETURNING id, filename, mime_type, size_bytes, storage_provider, storage_path, sha256, alt_text, uploaded_by, folder_id, created_at, updated_at"#
    )
    .bind(&body.alt_text)
    .bind(&body.filename)
    .bind(resolved_folder_id)
    .bind(&new_storage_path)
    .bind(id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to update file metadata: {}", e),
    })?
    .ok_or_else(|| AppError::NotFound(format!("File '{}' not found", id)))?;

    Ok(Json(json!({
        "id": row.0,
        "filename": row.1,
        "mime_type": row.2,
        "size_bytes": row.3,
        "storage_provider": row.4,
        "storage_path": row.5,
        "sha256": row.6,
        "alt_text": row.7,
        "uploaded_by": row.8,
        "folder_id": row.9,
        "created_at": row.10,
        "updated_at": row.11,
        "download_url": format!("/api/files/{}/download", row.0),
    })))
}

pub async fn batch_delete_files(
    State(state): State<Arc<AppState>>,
    _headers: axum::http::HeaderMap,
    Json(body): Json<BatchDeleteFilesBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    let mut deleted: i64 = 0;
    let mut errors: Vec<String> = Vec::new();

    for id in &body.ids {
        let row = match sqlx::query_as::<_, (String,)>(
            "SELECT storage_path FROM file_metadata WHERE id = $1",
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

        match sqlx::query("DELETE FROM file_metadata WHERE id = $1")
            .bind(id)
            .execute(db_pool)
            .await
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
    let db_pool = state.db()?;
    let actor_id = extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(Uuid::nil());

    if let Some(pid) = body.parent_id {
        let parent_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM file_folders WHERE id = $1)",
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
        "SELECT EXISTS(SELECT 1 FROM file_folders WHERE name = $1 AND parent_id IS NOT DISTINCT FROM $2)",
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
        "SELECT EXISTS(SELECT 1 FROM file_metadata WHERE filename = $1 AND folder_id IS NOT DISTINCT FROM $2)",
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

    let row = sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<Uuid>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        r#"INSERT INTO file_folders (name, parent_id, created_by)
           VALUES ($1, $2, $3)
           RETURNING id, name, parent_id, created_by, created_at, updated_at"#
    )
    .bind(&body.name)
    .bind(body.parent_id)
    .bind(actor_id)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to create folder: {}", e),
    })?;

    Ok(Json(json!({
        "id": row.0,
        "name": row.1,
        "parent_id": row.2,
        "created_by": row.3,
        "created_at": row.4,
        "updated_at": row.5,
    })))
}

pub async fn list_folders(
    State(state): State<Arc<AppState>>,
    Query(params): Query<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;
    let parent_id = params.get("parent_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty());

    let rows = if let Some(pid) = parent_id {
        let pid = Uuid::parse_str(pid).map_err(|_| AppError::BadRequest("Invalid parent_id UUID".to_string()))?;
        sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<Uuid>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
            "SELECT id, name, parent_id, created_by, created_at, updated_at FROM file_folders WHERE parent_id = $1 ORDER BY name ASC",
        )
        .bind(pid)
        .fetch_all(db_pool)
        .await
    } else {
        sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<Uuid>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
            "SELECT id, name, parent_id, created_by, created_at, updated_at FROM file_folders WHERE parent_id IS NULL ORDER BY name ASC",
        )
        .fetch_all(db_pool)
        .await
    }
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to list folders: {}", e),
    })?;

    let data: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.0,
                "name": r.1,
                "parent_id": r.2,
                "created_by": r.3,
                "created_at": r.4,
                "updated_at": r.5,
            })
        })
        .collect();

    Ok(Json(json!({ "data": data })))
}

pub async fn get_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    let folder = sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<Uuid>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, name, parent_id, created_by, created_at, updated_at FROM file_folders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to query folder: {}", e),
    })?
    .ok_or_else(|| AppError::NotFound("Folder not found".to_string()))?;

    let path = build_folder_path(db_pool, id).await?;

    let subfolder_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM file_folders WHERE parent_id = $1",
    )
    .bind(id)
    .fetch_one(db_pool)
    .await
    .unwrap_or(0);

    let file_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM file_metadata WHERE folder_id = $1",
    )
    .bind(id)
    .fetch_one(db_pool)
    .await
    .unwrap_or(0);

    Ok(Json(json!({
        "id": folder.0,
        "name": folder.1,
        "parent_id": folder.2,
        "created_by": folder.3,
        "created_at": folder.4,
        "updated_at": folder.5,
        "path": path,
        "subfolder_count": subfolder_count,
        "file_count": file_count,
    })))
}

pub async fn update_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateFolderBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    let current = sqlx::query_as::<_, (String, Option<Uuid>)>(
        "SELECT name, parent_id FROM file_folders WHERE id = $1",
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
            "SELECT EXISTS(SELECT 1 FROM file_folders WHERE name = $1 AND parent_id IS NOT DISTINCT FROM $2 AND id != $3)",
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
                "A folder named '{}' already exists in the target location", new_name
            )));
        }
    }

    if body.name.is_some() && body.name.as_deref() != Some(&old_name) {
        let sibling_conflict = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM file_folders WHERE name = $1 AND parent_id IS NOT DISTINCT FROM $2 AND id != $3)",
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
                "A folder named '{}' already exists in this location", new_name
            )));
        }
    }

    sqlx::query(
        "UPDATE file_folders SET name = $1, parent_id = $2, updated_at = NOW() WHERE id = $3",
    )
    .bind(new_name)
    .bind(new_parent_id)
    .bind(id)
    .execute(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to update folder: {}", e),
    })?;

    let new_path = build_folder_path(db_pool, id).await?;

    if old_path != new_path {
        let mut all_folder_ids = vec![id];
        let mut queue = vec![id];
        while let Some(fid) = queue.pop() {
            let children: Vec<Uuid> = sqlx::query_scalar(
                "SELECT id FROM file_folders WHERE parent_id = $1",
            )
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

            let files: Vec<(Uuid, String)> = sqlx::query_as(
                "SELECT id, filename FROM file_metadata WHERE folder_id = $1",
            )
            .bind(fid)
            .fetch_all(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to query folder files: {}", e),
            })?;

            for (file_id, file_name) in files {
                let new_storage_path = if folder_path.is_empty() {
                    format!("{}-{}", Uuid::new_v4(), file_name)
                } else {
                    format!("{}/{}", folder_path, file_name)
                };

                let old_storage_path: String = sqlx::query_scalar(
                    "SELECT storage_path FROM file_metadata WHERE id = $1",
                )
                .bind(file_id)
                .fetch_optional(db_pool)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to query file storage_path: {}", e),
                })?
                .unwrap_or_default();

                if let Ok(Some((mime, data))) = state.file_storage.download(&old_storage_path).await {
                    let _ = state.file_storage.delete(&old_storage_path).await;
                    let _ = state.file_storage.upload(
                        data,
                        &mime,
                        &file_name,
                        if folder_path.is_empty() { None } else { Some(&folder_path) },
                    ).await;
                }

                sqlx::query("UPDATE file_metadata SET storage_path = $1, updated_at = NOW() WHERE id = $2")
                    .bind(&new_storage_path)
                    .bind(file_id)
                    .execute(db_pool)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Failed to update file storage_path: {}", e),
                    })?;
            }
        }
    }

    let updated = sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<Uuid>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, name, parent_id, created_by, created_at, updated_at FROM file_folders WHERE id = $1",
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
    Query(params): Query<DeleteFolderQuery>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let db_pool = state.db()?;
    let recursive = params.recursive.unwrap_or(false);

    let folder_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM file_folders WHERE id = $1)",
    )
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
        let children: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM file_folders WHERE parent_id = $1",
        )
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

    let file_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM file_metadata WHERE folder_id = ANY($1)",
    )
    .bind(&all_folder_ids)
    .fetch_one(db_pool)
    .await
    .unwrap_or(0);

    if file_count > 0 && !recursive {
        return Err(AppError::BadRequest(
            format!("Folder is not empty ({} files). Use recursive=true to delete.", file_count)
        ));
    }

    if recursive {
        for &fid in &all_folder_ids {
            let files: Vec<(Uuid, String)> = sqlx::query_as(
                "SELECT id, storage_path FROM file_metadata WHERE folder_id = $1",
            )
            .bind(fid)
            .fetch_all(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to query files in folder: {}", e),
            })?;

            for (file_id, storage_path) in files {
                let _ = state.file_storage.delete(&storage_path).await;
                let _ = sqlx::query("DELETE FROM file_metadata WHERE id = $1")
                    .bind(file_id)
                    .execute(db_pool)
                    .await;
            }
        }

        for &fid in all_folder_ids.iter().rev() {
            sqlx::query("DELETE FROM file_folders WHERE id = $1")
                .bind(fid)
                .execute(db_pool)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to delete folder: {}", e),
                })?;
        }
    }

    if !recursive {
        sqlx::query("DELETE FROM file_folders WHERE id = $1")
            .bind(id)
            .execute(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to delete folder: {}", e),
            })?;
    }

    Ok(StatusCode::NO_CONTENT)
}
