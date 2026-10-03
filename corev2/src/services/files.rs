use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use file_storage::FileStorage;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    AppState, item_map,
    middelware::auth::AuthLevel,
    services::{
        auth::AuthService,
        collections::schema::get_pk_key,
        config::Config,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        hooks::{
            HookContext,
            types::files::{FileDeleted, FileUploaded},
        },
        items::{
            query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query},
            service::ItemsService,
        },
        permissions::read::{ReadAccess, resolve_access},
        scopes::require_scope,
        versions::VersionsService,
    },
};

pub const FILE_METADATA: &str = "alcedocore_file_metadata";
pub const FILE_FOLDERS: &str = "alcedocore_file_folders";
pub const ITEM_FILES: &str = "alcedocore_item_files";

/// Builds the configured storage backend. S3 is compiled only with `--features s3`.
pub async fn build_file_storage(config: &Config) -> Arc<dyn FileStorage> {
    match config.file_storage_provider.as_str() {
        "s3" => {
            #[cfg(feature = "s3")]
            {
                let bucket = config
                    .s3_bucket
                    .clone()
                    .expect("S3_BUCKET is required when FILE_STORAGE_PROVIDER=s3");
                let storage = file_storage_s3::S3FileStorage::new(&bucket, &config.s3_prefix)
                    .await
                    .expect("Failed to initialize S3 file storage");
                Arc::new(storage)
            }
            #[cfg(not(feature = "s3"))]
            {
                panic!("FILE_STORAGE_PROVIDER=s3 requires building with --features s3");
            }
        }
        _ => {
            // A missing directory is expected on a fresh checkout; create it
            // rather than panicking on a permission/not-found error.
            std::fs::create_dir_all(&config.files_dir).unwrap_or_else(|e| {
                panic!(
                    "Failed to create local file storage directory '{}': {}",
                    config.files_dir, e
                )
            });
            let storage = file_storage_local::LocalFileStorage::new(&config.files_dir)
                .expect("Failed to initialize local file storage");
            Arc::new(storage)
        }
    }
}

/// The path half of a file's `download_url` — no scheme/host, so a same-origin
/// `<img>` can use it directly and the `<app>/<version>` context survives on
/// requests (`<img>`) that cannot send the `X-App`/`X-Version` headers.
pub fn download_url_path(ctx: &AppContext, id: &Uuid) -> String {
    format!(
        "/api/app/files/{}/download?ac_app={}&ac_version={}",
        id,
        ctx.app_api_name(),
        ctx.version_api_name()
    )
}

fn field_cmp(field: &str, cmp: Comparison) -> Filter {
    let mut fields = HashMap::new();
    fields.insert(field.to_string(), FieldValue::Comparison(cmp));
    Filter::Field(FieldFilter { fields })
}

fn filter_all(conditions: Vec<Filter>) -> LogicOp {
    LogicOp {
        _and: if conditions.is_empty() {
            None
        } else {
            Some(conditions)
        },
        _or: None,
    }
}

fn file_metadata_json(row: &Map<String, Value>, ctx: &AppContext, id: Uuid) -> Value {
    let get = |key: &str| row.get(key).cloned().unwrap_or(Value::Null);
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
        "download_url": download_url_path(ctx, &id),
    })
}

fn folder_json(row: &Map<String, Value>) -> Value {
    let get = |key: &str| row.get(key).cloned().unwrap_or(Value::Null);
    json!({
        "id": get("id"),
        "name": get("name"),
        "parent_id": get("parent_id"),
        "created_by": get("created_by"),
        "created_at": get("created_at"),
        "updated_at": get("updated_at"),
    })
}

fn row_uuid(row: &Map<String, Value>, key: &str) -> Option<Uuid> {
    row.get(key)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
}

/// Query parameters for `GET /api/app/files`.
#[derive(Debug, Default, serde::Deserialize, utoipa::ToSchema)]
pub struct ListFilesParams {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub search: Option<String>,
    pub mime_type: Option<String>,
    pub folder_id: Option<String>,
}

pub struct FilesService<'a> {
    state: &'a AppState,
    context: &'a AppContext,
    metadata: String,
    folders: String,
}

impl<'a> FilesService<'a> {
    pub fn new(state: &'a AppState, context: &'a AppContext) -> Self {
        FilesService {
            state,
            context,
            metadata: FILE_METADATA.to_string(),
            folders: FILE_FOLDERS.to_string(),
        }
    }

    fn metadata_svc(&self) -> ItemsService<'_> {
        ItemsService::new(self.state, self.context, &self.metadata)
    }

    /// Emits a file hook with no transaction (called after the metadata write
    /// has committed, so subscribers observe the durable row).
    async fn emit_file_event<T: Send + Sync + 'static>(&self, key: &str, event: &mut T) {
        let hook_context = HookContext {
            context: self.context.clone(),
            state: self.state.clone(),
            tx: None,
        };
        self.state.event_bus.trigger(key, event, hook_context).await;
    }

    fn folders_svc(&self) -> ItemsService<'_> {
        ItemsService::new(self.state, self.context, &self.folders)
    }

    async fn find_metadata(&self, id: Uuid) -> Result<Option<Map<String, Value>>, AlcedoError> {
        Ok(self
            .metadata_svc()
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("id", Value::String(id.to_string()))
            })
            .await?
            .into_iter()
            .next())
    }

    async fn find_folder(&self, id: Uuid) -> Result<Option<Map<String, Value>>, AlcedoError> {
        Ok(self
            .folders_svc()
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("id", Value::String(id.to_string()))
            })
            .await?
            .into_iter()
            .next())
    }

    /// `{version_id}/{app_id}` namespace so identically-named files in
    /// different apps/versions never collide in one storage backend.
    async fn storage_prefix(&self) -> Result<String, AlcedoError> {
        let version_id = VersionsService::new(self.state)
            .get_version_id_by_name(&self.context.version)
            .await?
            .ok_or_else(|| {
                AlcedoError::NotFound(
                    format!("Version not found: {}", self.context.version),
                    0,
                )
            })?;

        let system = AppContext::system(RequestSource::API);
        let collection = "alcedo_apps".to_string();
        let apps = ItemsService::new(self.state, &system, &collection);
        let rows = apps
            .read_items_by_query(Query {
                fields: vec!["id".to_string()],
                limit: 0,
                ..Query::eq(
                    "api_name",
                    Value::String(self.context.app_api_name()),
                )
            })
            .await?;
        let app_id = rows
            .first()
            .and_then(|row| row.get("id"))
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                AlcedoError::NotFound(format!("App not found: {}", self.context.app_name), 0)
            })?;

        Ok(format!("{}/{}", version_id, app_id))
    }

    async fn object_folder(&self, folder_path: &str) -> Result<String, AlcedoError> {
        let base = self.storage_prefix().await?;
        Ok(if folder_path.is_empty() {
            base
        } else {
            format!("{}/{}", base, folder_path)
        })
    }

    /// Wallet of folder names from the root down to `folder_id`.
    pub async fn build_folder_path(&self, folder_id: Uuid) -> Result<String, AlcedoError> {
        let mut parts: Vec<String> = Vec::new();
        let mut current = Some(folder_id);
        while let Some(fid) = current {
            let row = self.find_folder(fid).await?.ok_or_else(|| {
                AlcedoError::NotFound("Folder not found".to_string(), 0)
            })?;
            let name = row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            parts.push(name);
            current = row_uuid(&row, "parent_id");
        }
        parts.reverse();
        Ok(parts.join("/"))
    }

    async fn folder_children(&self, folder_id: Uuid) -> Result<Vec<Uuid>, AlcedoError> {
        let rows = self
            .folders_svc()
            .read_items_by_query(Query {
                fields: vec!["id".to_string()],
                limit: 0,
                ..Query::eq("parent_id", Value::String(folder_id.to_string()))
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|row| row_uuid(row, "id"))
            .collect())
    }

    /// `folder_id` plus every descendant, breadth-first.
    async fn folder_subtree(&self, folder_id: Uuid) -> Result<Vec<Uuid>, AlcedoError> {
        let mut all = vec![folder_id];
        let mut queue = vec![folder_id];
        while let Some(fid) = queue.pop() {
            for child in self.folder_children(fid).await? {
                all.push(child);
                queue.push(child);
            }
        }
        Ok(all)
    }

    async fn folder_files(&self, folder_id: Uuid) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        self.metadata_svc()
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("folder_id", Value::String(folder_id.to_string()))
            })
            .await
    }

    async fn file_conflict(
        &self,
        filename: &str,
        folder_id: Option<Uuid>,
        exclude: Option<Uuid>,
    ) -> Result<Option<Map<String, Value>>, AlcedoError> {
        let mut conditions = vec![field_cmp(
            "filename",
            Comparison {
                _eq: Some(json!(filename)),
                ..Default::default()
            },
        )];
        conditions.push(match folder_id {
            Some(fid) => field_cmp(
                "folder_id",
                Comparison {
                    _eq: Some(json!(fid.to_string())),
                    ..Default::default()
                },
            ),
            None => field_cmp(
                "folder_id",
                Comparison {
                    _null: Some(true),
                    ..Default::default()
                },
            ),
        });
        if let Some(excluded) = exclude {
            conditions.push(field_cmp(
                "id",
                Comparison {
                    _neq: Some(json!(excluded.to_string())),
                    ..Default::default()
                },
            ));
        }
        Ok(self
            .metadata_svc()
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                filter: filter_all(conditions),
                ..Default::default()
            })
            .await?
            .into_iter()
            .next())
    }

    async fn validate_no_circular_ref(
        &self,
        folder_id: Uuid,
        new_parent: Option<Uuid>,
    ) -> Result<(), AlcedoError> {
        let Some(parent) = new_parent else {
            return Ok(());
        };
        if parent == folder_id {
            return Err(AlcedoError::InvalidInput(
                "A folder cannot be its own parent".to_string(),
                0,
            ));
        }
        // Walk up from the prospective parent; if we reach `folder_id`, the move
        // would create a cycle.
        let mut current = Some(parent);
        while let Some(fid) = current {
            if fid == folder_id {
                return Err(AlcedoError::InvalidInput(
                    "A folder cannot be moved inside itself".to_string(),
                    0,
                ));
            }
            current = self
                .find_folder(fid)
                .await?
                .and_then(|row| row_uuid(&row, "parent_id"));
        }
        Ok(())
    }

    // ---- files ----

    pub async fn list_files(&self, params: &ListFilesParams) -> Result<Value, AlcedoError> {
        let mut conditions: Vec<Filter> = Vec::new();
        if let Some(search) = params.search.as_ref().filter(|s| !s.is_empty()) {
            conditions.push(field_cmp(
                "filename",
                Comparison {
                    _icontains: Some(search.clone()),
                    ..Default::default()
                },
            ));
        }
        if let Some(mime) = params.mime_type.as_ref().filter(|s| !s.is_empty()) {
            conditions.push(field_cmp(
                "mime_type",
                Comparison {
                    _istarts_with: Some(mime.clone()),
                    ..Default::default()
                },
            ));
        }
        match params.folder_id.as_deref() {
            Some("") => conditions.push(field_cmp(
                "folder_id",
                Comparison {
                    _null: Some(true),
                    ..Default::default()
                },
            )),
            Some(folder_id) => {
                let fid = Uuid::parse_str(folder_id).map_err(|_| {
                    AlcedoError::InvalidInput("Invalid folder_id".to_string(), 0)
                })?;
                conditions.push(field_cmp(
                    "folder_id",
                    Comparison {
                        _eq: Some(json!(fid.to_string())),
                        ..Default::default()
                    },
                ));
            }
            None => {}
        }

        let filter = filter_all(conditions);
        let rows = self
            .metadata_svc()
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                sort: vec!["-created_at".to_string()],
                limit: params.limit.unwrap_or(50),
                offset: params.offset.unwrap_or(0),
                filter: filter.clone(),
                ..Default::default()
            })
            .await?;
        let total = self
            .metadata_svc()
            .count_items_by_query(Query {
                filter,
                ..Default::default()
            })
            .await?;

        let data: Vec<Value> = rows
            .iter()
            .filter_map(|row| {
                let id = row_uuid(row, "id")?;
                Some(file_metadata_json(row, self.context, id))
            })
            .collect();

        Ok(json!({ "data": data, "total": total }))
    }

    pub async fn get_file(&self, id: Uuid) -> Result<Value, AlcedoError> {
        let row = self
            .find_metadata(id)
            .await?
            .ok_or_else(|| AlcedoError::NotFound(format!("File '{}' not found", id), 0))?;
        Ok(file_metadata_json(&row, self.context, id))
    }

    pub async fn download(&self, id: Uuid) -> Result<(String, Bytes), AlcedoError> {
        let row = self
            .find_metadata(id)
            .await?
            .ok_or_else(|| AlcedoError::NotFound(format!("File '{}' not found", id), 0))?;
        let storage_path = row
            .get("storage_path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AlcedoError::SystemError("Invalid file row: missing storage_path".to_string(), 0)
            })?
            .to_string();
        let mime = row
            .get("mime_type")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_string();

        match self.state.file_storage.download(&storage_path).await {
            Ok(Some((_stored_mime, bytes))) => Ok((mime, bytes)),
            Ok(None) => Err(AlcedoError::NotFound(
                format!("File '{}' not found in storage", id),
                0,
            )),
            Err(e) => Err(AlcedoError::SystemError(
                format!("File storage download failed: {}", e),
                0,
            )),
        }
    }

    /// True for global admins and developer keys, who bypass record-level file
    /// checks (mirrors the item-permission bypass).
    async fn is_privileged(&self) -> Result<bool, AlcedoError> {
        match self.context.identity.as_ref() {
            Some(AuthLevel::DeveloperKey { .. }) => Ok(true),
            Some(AuthLevel::User(user_id)) => {
                let system = AppContext::system(RequestSource::API);
                AuthService::new(self.state, &system).is_admin(*user_id).await
            }
            _ => Ok(false),
        }
    }

    /// The first item a file is linked to, as `(collection, field, item_id)`.
    async fn find_item_file_link(
        &self,
        file_id: Uuid,
    ) -> Result<Option<(String, String, String)>, AlcedoError> {
        let collection = ITEM_FILES.to_string();
        let service = ItemsService::new(self.state, self.context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec![
                    "collection_name".to_string(),
                    "field_name".to_string(),
                    "item_id".to_string(),
                ],
                limit: 0,
                ..Query::eq("file_id", Value::String(file_id.to_string()))
            })
            .await?;
        Ok(rows.into_iter().next().and_then(|row| {
            Some((
                row.get("collection_name")?.as_str()?.to_string(),
                row.get("field_name")?.as_str()?.to_string(),
                row.get("item_id")?.as_str()?.to_string(),
            ))
        }))
    }

    /// A file may be downloaded when the caller can read the field that
    /// references it on the linked record. Reading the record through the
    /// caller's context applies the row + field policies, so a hit means the
    /// field is readable. Admins/dev keys bypass; unlinked files are denied.
    pub async fn check_download_permission(&self, file_id: Uuid) -> Result<(), AlcedoError> {
        if self.is_privileged().await? {
            return Ok(());
        }
        let Some((collection, field, item_id)) = self.find_item_file_link(file_id).await? else {
            return Err(AlcedoError::Forbidden(
                "File is not linked to any record".to_string(),
                0,
            ));
        };

        let service = ItemsService::new(self.state, self.context, &collection);
        let pk = get_pk_key(
            &self.state.database_schema,
            &self.context.schema_name(),
            &collection,
        )
        .await?
        .name;
        let mut query = Query::eq(&pk, Value::String(item_id));
        query.fields = vec![field.clone()];
        query.limit = 1;
        let rows = service.read_items_by_query(query).await?;

        let wanted = file_id.to_string();
        let allowed = rows.iter().any(|row| match row.get(&field) {
            Some(Value::Array(values)) => values.iter().any(|v| v.as_str() == Some(&wanted)),
            Some(Value::String(value)) => value == &wanted,
            _ => false,
        });
        if allowed {
            Ok(())
        } else {
            Err(AlcedoError::Forbidden(
                "You do not have permission to download this file".to_string(),
                0,
            ))
        }
    }

    /// Uploads are allowed when the caller may create or update records in the
    /// target collection. With no target collection (e.g. the media library)
    /// this falls back to the collection-agnostic `items.write` scope.
    pub async fn check_upload_permission(
        &self,
        collection_name: Option<&str>,
    ) -> Result<(), AlcedoError> {
        if self.is_privileged().await? {
            return Ok(());
        }
        match collection_name.filter(|name| !name.is_empty()) {
            Some(collection) => {
                let identity = self.context.identity.as_ref();
                let create =
                    resolve_access(self.state, self.context, collection, identity, "create")
                        .await?;
                let update =
                    resolve_access(self.state, self.context, collection, identity, "update")
                        .await?;
                if !matches!(create, ReadAccess::Deny) || !matches!(update, ReadAccess::Deny) {
                    Ok(())
                } else {
                    Err(AlcedoError::Forbidden(
                        format!("Not allowed to upload files for '{}'", collection),
                        0,
                    ))
                }
            }
            None => {
                let auth = self
                    .context
                    .identity
                    .clone()
                    .unwrap_or(AuthLevel::Public);
                require_scope(self.state, &auth, self.context, "items.write").await
            }
        }
    }

    pub async fn upload(
        &self,
        data: Vec<u8>,
        filename: String,
        mime_type: String,
        folder_id: Option<Uuid>,
        overwrite: bool,
        uploaded_by: Option<Uuid>,
    ) -> Result<Value, AlcedoError> {
        let folder_path = match folder_id {
            Some(fid) => self.build_folder_path(fid).await?,
            None => String::new(),
        };

        let existing = self.file_conflict(&filename, folder_id, None).await?;
        if let Some(row) = existing {
            if !overwrite {
                return Err(AlcedoError::InvalidInput(
                    format!(
                        "A file named '{}' already exists in the target location. Set overwrite=true to replace.",
                        filename
                    ),
                    0,
                ));
            }
            if let Some(path) = row.get("storage_path").and_then(Value::as_str) {
                let _ = self.state.file_storage.delete(path).await;
            }
            if let Some(existing_id) = row_uuid(&row, "id") {
                let mut svc = self.metadata_svc();
                svc.delete_items_by_pks(vec![Value::String(existing_id.to_string())], None)
                    .await?;
            }
        }

        let object_folder = self.object_folder(&folder_path).await?;
        let size = data.len() as i64;
        let storage_path = self
            .state
            .file_storage
            .upload(Bytes::from(data), &mime_type, &filename, &object_folder)
            .await
            .map_err(|e| {
                AlcedoError::SystemError(format!("File storage upload failed: {}", e), 0)
            })?;

        let id = Uuid::new_v4();
        let mut payload = item_map! {
            "id" => id.to_string(),
            "filename" => filename,
            "mime_type" => mime_type,
            "size_bytes" => size,
            "storage_provider" => self.state.config.file_storage_provider,
            "storage_path" => storage_path,
            "sha256" => Value::Null,
            "alt_text" => Value::Null,
        };
        if let Some(fid) = folder_id {
            payload.insert("folder_id".to_string(), json!(fid.to_string()));
        }
        if let Some(uid) = uploaded_by {
            payload.insert("uploaded_by".to_string(), json!(uid.to_string()));
        }

        let mut svc = self.metadata_svc();
        svc.create_many(vec![payload], &mut None).await?;

        let result = self.get_file(id).await?;
        let mut event = FileUploaded {
            file_id: id,
            filename: result
                .get("filename")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            mime_type: result
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            size_bytes: result.get("size_bytes").and_then(Value::as_i64).unwrap_or(0),
        };
        self.emit_file_event("file.uploaded", &mut event).await;

        Ok(result)
    }

    pub async fn update_file(
        &self,
        id: Uuid,
        filename: Option<String>,
        folder_id: Option<Option<Uuid>>,
        alt_text: Option<Option<String>>,
    ) -> Result<Value, AlcedoError> {
        let current = self
            .find_metadata(id)
            .await?
            .ok_or_else(|| AlcedoError::NotFound(format!("File '{}' not found", id), 0))?;

        let current_filename = current
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let current_folder = row_uuid(&current, "folder_id");
        let current_storage = current
            .get("storage_path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let new_filename = filename.unwrap_or_else(|| current_filename.clone());
        let new_folder = folder_id.unwrap_or(current_folder);
        let folder_changed = new_folder != current_folder;
        let name_changed = new_filename != current_filename;

        if name_changed || folder_changed {
            if let Some(conflict) = self
                .file_conflict(&new_filename, new_folder, Some(id))
                .await?
            {
                if conflict.get("id").and_then(Value::as_str) != Some(&id.to_string()) {
                    return Err(AlcedoError::InvalidInput(
                        format!("A file named '{}' already exists", new_filename),
                        0,
                    ));
                }
            }
        }

        let mut payload: Map<String, Value> = match alt_text {
            Some(alt) => item_map! {
                "alt_text" => alt.map(Value::String).unwrap_or(Value::Null),
            },
            None => Default::default(),
        };

        if name_changed || folder_changed {
            let folder_path = match new_folder {
                Some(fid) => self.build_folder_path(fid).await?,
                None => String::new(),
            };
            let object_folder = self.object_folder(&folder_path).await?;
            let mime = current
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream")
                .to_string();
            if let Ok(Some((_m, bytes))) = self.state.file_storage.download(&current_storage).await {
                let _ = self.state.file_storage.delete(&current_storage).await;
                let new_storage = self
                    .state
                    .file_storage
                    .upload(bytes, &mime, &new_filename, &object_folder)
                    .await
                    .map_err(|e| {
                        AlcedoError::SystemError(
                            format!("File storage upload failed: {}", e),
                            0,
                        )
                    })?;
                payload.insert("storage_path".to_string(), json!(new_storage));
            }
            payload.insert("filename".to_string(), json!(new_filename));
            payload.insert(
                "folder_id".to_string(),
                new_folder.map(|f| json!(f.to_string())).unwrap_or(Value::Null),
            );
        }

        if !payload.is_empty() {
            let mut svc = self.metadata_svc();
            let mut query = Query::eq("id", Value::String(id.to_string()));
            svc.update_items_by_query(&mut query, payload, &mut None)
                .await?;
        }

        self.get_file(id).await
    }

    pub async fn delete_file(&self, id: Uuid) -> Result<(), AlcedoError> {
        let row = self
            .find_metadata(id)
            .await?
            .ok_or_else(|| AlcedoError::NotFound(format!("File '{}' not found", id), 0))?;
        if let Some(path) = row.get("storage_path").and_then(Value::as_str) {
            let _ = self.state.file_storage.delete(path).await;
        }
        let mut svc = self.metadata_svc();
        svc.delete_items_by_pks(vec![Value::String(id.to_string())], None)
            .await?;

        let mut event = FileDeleted {
            file_id: id,
            filename: row
                .get("filename")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        self.emit_file_event("file.deleted", &mut event).await;
        Ok(())
    }

    pub async fn batch_delete_files(&self, ids: Vec<Uuid>) -> Result<Value, AlcedoError> {
        let mut deleted = 0u64;
        for id in ids {
            if self.delete_file(id).await.is_ok() {
                deleted += 1;
            }
        }
        Ok(json!({ "success": true, "count": deleted }))
    }

    // ---- folders ----

    pub async fn list_folders(&self, parent_id: Option<Uuid>) -> Result<Value, AlcedoError> {
        let filter = match parent_id {
            Some(pid) => filter_all(vec![field_cmp(
                "parent_id",
                Comparison {
                    _eq: Some(json!(pid.to_string())),
                    ..Default::default()
                },
            )]),
            None => filter_all(vec![field_cmp(
                "parent_id",
                Comparison {
                    _null: Some(true),
                    ..Default::default()
                },
            )]),
        };
        let rows = self
            .folders_svc()
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                sort: vec!["+name".to_string()],
                limit: 0,
                filter,
                ..Default::default()
            })
            .await?;
        let data: Vec<Value> = rows.iter().map(folder_json).collect();
        Ok(json!({ "data": data }))
    }

    pub async fn get_folder(&self, id: Uuid) -> Result<Value, AlcedoError> {
        let row = self
            .find_folder(id)
            .await?
            .ok_or_else(|| AlcedoError::NotFound("Folder not found".to_string(), 0))?;
        let path = self.build_folder_path(id).await?;
        let mut body = folder_json(&row);
        body["path"] = json!(path);

        let subfolders = self
            .folders_svc()
            .count_items_by_query(Query {
                filter: filter_all(vec![field_cmp(
                    "parent_id",
                    Comparison {
                        _eq: Some(json!(id.to_string())),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            })
            .await?;
        let files = self
            .metadata_svc()
            .count_items_by_query(Query {
                filter: filter_all(vec![field_cmp(
                    "folder_id",
                    Comparison {
                        _eq: Some(json!(id.to_string())),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            })
            .await?;
        body["subfolder_count"] = json!(subfolders);
        body["file_count"] = json!(files);
        Ok(body)
    }

    pub async fn create_folder(
        &self,
        name: &str,
        parent_id: Option<Uuid>,
        created_by: Option<Uuid>,
    ) -> Result<Value, AlcedoError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AlcedoError::InvalidInput(
                "Folder name is required".to_string(),
                0,
            ));
        }
        if let Some(pid) = parent_id {
            if self.find_folder(pid).await?.is_none() {
                return Err(AlcedoError::NotFound("Parent folder not found".to_string(), 0));
            }
        }
        if self.folder_name_conflict(name, parent_id, None).await? {
            return Err(AlcedoError::InvalidInput(
                format!("A folder named '{}' already exists", name),
                0,
            ));
        }

        let id = Uuid::new_v4();
        let payload = item_map! {
            "id" => id.to_string(),
            "name" => name,
            "parent_id" => parent_id.map(|p| json!(p.to_string())).unwrap_or(Value::Null),
            "created_by" => created_by.map(|u| json!(u.to_string())).unwrap_or(Value::Null),
        };

        let mut svc = self.folders_svc();
        svc.create_many(vec![payload], &mut None).await?;

        let row = self
            .find_folder(id)
            .await?
            .ok_or_else(|| AlcedoError::SystemError("Folder was not created".to_string(), 0))?;
        let mut body = folder_json(&row);
        body["path"] = json!(self.build_folder_path(id).await?);
        Ok(body)
    }

    async fn folder_name_conflict(
        &self,
        name: &str,
        parent_id: Option<Uuid>,
        exclude: Option<Uuid>,
    ) -> Result<bool, AlcedoError> {
        let mut conditions = vec![field_cmp(
            "name",
            Comparison {
                _eq: Some(json!(name)),
                ..Default::default()
            },
        )];
        conditions.push(match parent_id {
            Some(pid) => field_cmp(
                "parent_id",
                Comparison {
                    _eq: Some(json!(pid.to_string())),
                    ..Default::default()
                },
            ),
            None => field_cmp(
                "parent_id",
                Comparison {
                    _null: Some(true),
                    ..Default::default()
                },
            ),
        });
        if let Some(excluded) = exclude {
            conditions.push(field_cmp(
                "id",
                Comparison {
                    _neq: Some(json!(excluded.to_string())),
                    ..Default::default()
                },
            ));
        }
        let count = self
            .folders_svc()
            .count_items_by_query(Query {
                filter: filter_all(conditions),
                ..Default::default()
            })
            .await?;
        Ok(count > 0)
    }

    pub async fn update_folder(
        &self,
        id: Uuid,
        name: Option<String>,
        parent_id: Option<Option<Uuid>>,
    ) -> Result<Value, AlcedoError> {
        let current = self
            .find_folder(id)
            .await?
            .ok_or_else(|| AlcedoError::NotFound("Folder not found".to_string(), 0))?;
        let old_name = current
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let old_parent = row_uuid(&current, "parent_id");
        let old_path = self.build_folder_path(id).await?;

        let new_name = name.clone().unwrap_or_else(|| old_name.clone());
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return Err(AlcedoError::InvalidInput(
                "Folder name is required".to_string(),
                0,
            ));
        }
        let new_parent = parent_id.unwrap_or(old_parent);

        if parent_id.is_some() {
            self.validate_no_circular_ref(id, new_parent).await?;
        }
        if self
            .folder_name_conflict(&new_name, new_parent, Some(id))
            .await?
        {
            return Err(AlcedoError::InvalidInput(
                format!("A folder named '{}' already exists", new_name),
                0,
            ));
        }

        let payload = item_map! {
            "name" => new_name,
            "parent_id" => new_parent.map(|p| json!(p.to_string())).unwrap_or(Value::Null),
        };
        let mut svc = self.folders_svc();
        let mut query = Query::eq("id", Value::String(id.to_string()));
        svc.update_items_by_query(&mut query, payload, &mut None)
            .await?;

        // If the tree shape changed, relocate every affected file in storage.
        let new_path = self.build_folder_path(id).await?;
        if old_path != new_path {
            self.relocate_subtree(id).await?;
        }

        let row = self
            .find_folder(id)
            .await?
            .ok_or_else(|| AlcedoError::NotFound("Folder not found".to_string(), 0))?;
        let mut body = folder_json(&row);
        body["path"] = json!(new_path);
        Ok(body)
    }

    async fn relocate_subtree(&self, root: Uuid) -> Result<(), AlcedoError> {
        for fid in self.folder_subtree(root).await? {
            let folder_path = self.build_folder_path(fid).await?;
            for file in self.folder_files(fid).await? {
                let Some(file_id) = row_uuid(&file, "id") else {
                    continue;
                };
                let old_storage = file
                    .get("storage_path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let filename = file
                    .get("filename")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let mime = file
                    .get("mime_type")
                    .and_then(Value::as_str)
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let object_folder = self.object_folder(&folder_path).await?;

                if let Ok(Some((_m, bytes))) = self.state.file_storage.download(&old_storage).await
                {
                    let _ = self.state.file_storage.delete(&old_storage).await;
                    let new_storage = self
                        .state
                        .file_storage
                        .upload(bytes, &mime, &filename, &object_folder)
                        .await
                        .map_err(|e| {
                            AlcedoError::SystemError(
                                format!("File storage upload failed: {}", e),
                                0,
                            )
                        })?;
                    let payload = item_map! {
                        "storage_path" => new_storage,
                    };
                    let mut svc = self.metadata_svc();
                    let mut query = Query::eq("id", Value::String(file_id.to_string()));
                    svc.update_items_by_query(&mut query, payload, &mut None)
                        .await?;
                }
            }
        }
        Ok(())
    }

    pub async fn delete_folder(&self, id: Uuid, recursive: bool) -> Result<(), AlcedoError> {
        if self.find_folder(id).await?.is_none() {
            return Err(AlcedoError::NotFound("Folder not found".to_string(), 0));
        }

        let subtree = self.folder_subtree(id).await?;
        let mut file_count = 0u64;
        for fid in &subtree {
            file_count += self.folder_files(*fid).await?.len() as u64;
        }
        if file_count > 0 && !recursive {
            return Err(AlcedoError::InvalidInput(
                format!(
                    "Folder is not empty ({} files). Use recursive=true to delete.",
                    file_count
                ),
                0,
            ));
        }

        if recursive {
            // Delete files first (storage + rows), then folders bottom-up.
            for fid in &subtree {
                for file in self.folder_files(*fid).await? {
                    if let Some(path) = file.get("storage_path").and_then(Value::as_str) {
                        let _ = self.state.file_storage.delete(path).await;
                    }
                    if let Some(file_id) = row_uuid(&file, "id") {
                        let mut svc = self.metadata_svc();
                        let _ = svc
                            .delete_items_by_pks(
                                vec![Value::String(file_id.to_string())],
                                None,
                            )
                            .await;
                    }
                }
            }
            let mut svc = self.folders_svc();
            for fid in subtree.iter().rev() {
                svc.delete_items_by_pks(vec![Value::String(fid.to_string())], None)
                    .await?;
            }
        } else {
            let mut svc = self.folders_svc();
            svc.delete_items_by_pks(vec![Value::String(id.to_string())], None)
                .await?;
        }
        Ok(())
    }

    // ---- item read augmentation ----

    /// Replaces `file`-type field UUID arrays with file metadata objects (incl.
    /// `download_url`) resolved against **this** context's schema, so a
    /// relational read from another app returns the owning app's paths.
    pub async fn augment_file_fields(
        &self,
        collection: &str,
        items: Vec<Map<String, Value>>,
    ) -> Vec<Map<String, Value>> {
        let fields = match crate::services::collections::get_collection(
            self.state,
            self.context,
            collection,
        )
        .await
        {
            Ok(collection) => collection.fields,
            Err(_) => return items,
        };
        let file_fields: Vec<String> = fields
            .iter()
            .filter(|f| f.field_type == "file")
            .map(|f| f.name.clone())
            .collect();
        if file_fields.is_empty() {
            return items;
        }

        let mut ids: Vec<Uuid> = Vec::new();
        for item in &items {
            for field in &file_fields {
                if let Some(Value::Array(values)) = item.get(field) {
                    for value in values {
                        if let Some(s) = value.as_str() {
                            if let Ok(id) = Uuid::parse_str(s) {
                                if !ids.contains(&id) {
                                    ids.push(id);
                                }
                            }
                        }
                    }
                }
            }
        }
        if ids.is_empty() {
            return items;
        }

        let wanted: Vec<Value> = ids.iter().map(|id| json!(id.to_string())).collect();
        let mut map: HashMap<String, Value> = HashMap::new();
        if let Ok(rows) = self
            .metadata_svc()
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                filter: filter_all(vec![field_cmp(
                    "id",
                    Comparison {
                        _in: Some(json!(wanted)),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            })
            .await
        {
            for row in rows {
                if let Some(id) = row_uuid(&row, "id") {
                    map.insert(id.to_string(), file_metadata_json(&row, self.context, id));
                }
            }
        }

        let mut augmented = items;
        for item in augmented.iter_mut() {
            for field in &file_fields {
                if let Some(Value::Array(values)) = item.get(field).cloned() {
                    let metadata: Vec<Value> = values
                        .iter()
                        .filter_map(|value| {
                            value
                                .as_str()
                                .and_then(|s| map.get(s).cloned())
                        })
                        .collect();
                    if !metadata.is_empty() {
                        item.insert(field.clone(), Value::Array(metadata));
                    }
                }
            }
        }
        augmented
    }
}
