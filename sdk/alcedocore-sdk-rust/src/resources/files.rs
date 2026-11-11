use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{FileListResponse, MediaFile};
use reqwest::Method;
use serde_json::Value;

pub struct FilesResource {
    client: BaseClient,
}

/// Optional metadata for an upload: `POST /api/app/files/upload` (multipart).
#[derive(Default, Clone)]
pub struct UploadOptions {
    pub collection_name: Option<String>,
    pub item_id: Option<String>,
    pub field_name: Option<String>,
    pub folder_id: Option<String>,
    pub overwrite: bool,
}

impl_new!(FilesResource);

impl FilesResource {
    /// Upload a file: `POST /api/app/files/upload` as multipart form data.
    /// `file_bytes` is sent as the `file` part with the given `filename`.
    pub async fn upload(
        &self,
        file_bytes: Vec<u8>,
        filename: &str,
        options: Option<UploadOptions>,
    ) -> Result<Value, AlcedoError> {
        let opts = options.unwrap_or_default();
        let part = reqwest::multipart::Part::bytes(file_bytes).file_name(filename.to_string());
        let mut form = reqwest::multipart::Form::new().part("file", part);
        if let Some(v) = opts.collection_name {
            form = form.text("collection_name", v);
        }
        if let Some(v) = opts.item_id {
            form = form.text("item_id", v);
        }
        if let Some(v) = opts.field_name {
            form = form.text("field_name", v);
        }
        if let Some(v) = opts.folder_id {
            form = form.text("folder_id", v);
        }
        if opts.overwrite {
            form = form.text("overwrite", "true");
        }
        let builder = self.client.request(Method::POST, "/api/app/files/upload").multipart(form);
        self.client.execute(builder).await
    }

    /// List files: `GET /api/app/files` (+ optional query params).
    pub async fn list(&self, params: Option<&[(&str, &str)]>) -> Result<FileListResponse, AlcedoError> {
        let mut builder = self.client.request(Method::GET, "/api/app/files");
        if let Some(p) = params {
            builder = builder.query(p);
        }
        self.client.execute_json(builder).await
    }

    /// Get file metadata: `GET /api/app/files/{id}`.
    pub async fn get(&self, id: &str) -> Result<MediaFile, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/files/{id}"));
        self.client.execute_json(builder).await
    }

    /// Update file metadata: `PATCH /api/app/files/{id}`.
    pub async fn update(&self, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PATCH, &format!("/api/app/files/{id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a file: `DELETE /api/app/files/{id}`.
    pub async fn delete(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::DELETE, &format!("/api/app/files/{id}"));
        self.client.execute(builder).await
    }

    /// Delete multiple files: `POST /api/app/files/batch/delete` with `{ids}`.
    pub async fn batch_delete(&self, ids: &[String]) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/app/files/batch/delete")
            .json(&serde_json::json!({ "ids": ids }));
        self.client.execute(builder).await
    }

    /// Download a file's raw bytes: `GET /api/app/files/{id}/download`.
    pub async fn download(&self, id: &str) -> Result<Vec<u8>, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/files/{id}/download"));
        let response = self.client.send(builder).await?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|e| AlcedoError::Connection {
            message: format!("Failed to read download body: {}", e),
            status_code: Some(status),
            source: Some(Box::new(e)),
        })?;
        if (200..300).contains(&status) {
            Ok(bytes.to_vec())
        } else {
            Err(AlcedoError::from_status(status, String::from_utf8_lossy(&bytes).to_string()))
        }
    }

    /// Build the download URL for a file id (for `<img src>` / links).
    /// Appends `ac_app`/`ac_version` query params when the client has them set.
    pub fn download_url(&self, id: &str) -> String {
        let mut url = format!("{}/api/app/files/{}/download", self.client.base_url(), id);
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(app) = self.client.app() {
            params.push(("ac_app".to_string(), app.to_string()));
        }
        if let Some(version) = self.client.version() {
            params.push(("ac_version".to_string(), version.to_string()));
        }
        if !params.is_empty() {
            let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            url.push('?');
            url.push_str(&query.join("&"));
        }
        url
    }

    // ─── Folders ────────────────────────────────────────────────────────────

    /// List folders: `GET /api/app/files/folders` (+ optional `parent_id`).
    pub async fn list_folders(&self, parent_id: Option<&str>) -> Result<Value, AlcedoError> {
        let mut builder = self.client.request(Method::GET, "/api/app/files/folders");
        if let Some(p) = parent_id {
            builder = builder.query(&[("parent_id", p)]);
        }
        self.client.execute(builder).await
    }

    /// Create a folder: `POST /api/app/files/folders` with `{name, parent_id?}`.
    pub async fn create_folder(&self, name: &str, parent_id: Option<&str>) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/app/files/folders")
            .json(&serde_json::json!({ "name": name, "parent_id": parent_id }));
        self.client.execute(builder).await
    }

    /// Get a folder: `GET /api/app/files/folders/{id}`.
    pub async fn get_folder(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/files/folders/{id}"));
        self.client.execute(builder).await
    }

    /// Update a folder: `PATCH /api/app/files/folders/{id}`.
    pub async fn update_folder(&self, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PATCH, &format!("/api/app/files/folders/{id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a folder: `DELETE /api/app/files/folders/{id}` (+ optional `recursive=true`).
    pub async fn delete_folder(&self, id: &str, recursive: bool) -> Result<Value, AlcedoError> {
        let mut builder = self
            .client
            .request(Method::DELETE, &format!("/api/app/files/folders/{id}"));
        if recursive {
            builder = builder.query(&[("recursive", "true")]);
        }
        self.client.execute(builder).await
    }
}
