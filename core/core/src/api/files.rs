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
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateFileMetadataBody {
    pub alt_text: Option<String>,
    pub filename: Option<String>,
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

    use sha2::Digest;
    let sha256 = hex::encode(sha2::Sha256::digest(&file_data));

    let storage_path = state
        .file_storage
        .upload(bytes::Bytes::from(file_data.clone()), &mime_type, &filename)
        .await
        .map_err(|e| AppError::Internal(format!("File storage upload failed: {}", e)))?;

    let row = sqlx::query_as::<_, (
        Uuid, String, String, i64, String, String, Option<String>, Option<String>, Option<Uuid>,
        chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        r#"INSERT INTO file_metadata (filename, mime_type, size_bytes, storage_provider, storage_path, sha256, uploaded_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id, filename, mime_type, size_bytes, storage_provider, storage_path, sha256, alt_text, uploaded_by, created_at, updated_at"#
    )
    .bind(&filename)
    .bind(&mime_type)
    .bind(file_data.len() as i64)
    .bind("local")
    .bind(&storage_path)
    .bind(&sha256)
    .bind(actor_id)
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
        "created_at": row.9,
        "updated_at": row.10,
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
        Uuid, String, String, i64, String, String, Option<String>, Option<String>, Option<Uuid>,
        chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        r#"SELECT id, filename, mime_type, size_bytes, storage_provider, storage_path, sha256, alt_text, uploaded_by, created_at, updated_at
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
        "created_at": row.9,
        "updated_at": row.10,
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
        where_conditions.push(format!("filename ILIKE ${}", bind_idx));
        bind_values.push(serde_json::Value::String(format!("%{}%", search)));
        bind_idx += 1;
    }

    if let Some(ref mime_type) = params.mime_type {
        where_conditions.push(format!("mime_type LIKE ${}", bind_idx));
        bind_values.push(serde_json::Value::String(format!("{}%", mime_type)));
        bind_idx += 1;
    }

    let where_clause = if where_conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM file_metadata {}", where_clause);
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
        r#"SELECT id, filename, mime_type, size_bytes, storage_provider, storage_path, sha256, alt_text, uploaded_by, created_at, updated_at
           FROM file_metadata {}
           ORDER BY created_at DESC
           LIMIT ${} OFFSET ${}"#,
        where_clause,
        bind_idx,
        bind_idx + 1,
    );
    bind_values.push(serde_json::Value::Number(serde_json::Number::from(limit)));
    bind_values.push(serde_json::Value::Number(serde_json::Number::from(offset)));

    let mut data_q = sqlx::query_as::<_, (
        Uuid, String, String, i64, String, String, Option<String>, Option<String>, Option<Uuid>,
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
                "created_at": row.9,
                "updated_at": row.10,
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

    let row = sqlx::query_as::<_, (
        Uuid, String, String, i64, String, String, Option<String>, Option<String>, Option<Uuid>,
        chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        r#"UPDATE file_metadata
           SET alt_text = COALESCE($1, alt_text),
               filename = COALESCE($2, filename),
               updated_at = NOW()
           WHERE id = $3
           RETURNING id, filename, mime_type, size_bytes, storage_provider, storage_path, sha256, alt_text, uploaded_by, created_at, updated_at"#
    )
    .bind(&body.alt_text)
    .bind(&body.filename)
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
        "created_at": row.9,
        "updated_at": row.10,
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
