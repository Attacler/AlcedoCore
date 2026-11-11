use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::resources::logs::LogListParams;
use reqwest::Method;
use serde_json::Value;

pub struct PluginsResource {
    client: BaseClient,
}

impl_new!(PluginsResource);

fn slug_path(slug: &str, suffix: &str) -> String {
    format!("/api/platform/plugins/{slug}{suffix}")
}

fn install_path(slug: &str, app_version_id: i64, suffix: &str) -> String {
    slug_path(slug, &format!("/installs/{app_version_id}{suffix}"))
}

fn install_id_path(install_id: i64, suffix: &str) -> String {
    format!("/api/platform/plugins/installs/{install_id}{suffix}")
}

impl PluginsResource {
    /// List plugins: `GET /api/platform/plugins`.
    pub async fn list(&self) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/plugins");
        self.client.execute(builder).await
    }

    /// Create a plugin: `POST /api/platform/plugins`.
    pub async fn create(&self, data: Value) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::POST, "/api/platform/plugins").json(&data);
        self.client.execute(builder).await
    }

    /// Get a plugin by slug: `GET /api/platform/plugins/{slug}`.
    pub async fn get(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &slug_path(slug, ""));
        self.client.execute(builder).await
    }

    /// Update a plugin: `PUT /api/platform/plugins/{slug}`.
    pub async fn update(&self, slug: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &slug_path(slug, ""))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a plugin: `DELETE /api/platform/plugins/{slug}`.
    pub async fn delete(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::DELETE, &slug_path(slug, ""));
        self.client.execute(builder).await
    }

    // ─── Deploy ─────────────────────────────────────────────────────────────

    /// Deploy a plugin: `POST /api/platform/plugins/deploy`.
    pub async fn deploy(&self, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/plugins/deploy")
            .json(&data);
        self.client.execute(builder).await
    }

    /// Preview a plugin image's manifest: `POST /api/platform/plugins/preview`.
    pub async fn preview(&self, image: &str, registry_id: Option<i64>) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/plugins/preview")
            .json(&serde_json::json!({ "image": image, "registry_id": registry_id }));
        self.client.execute(builder).await
    }

    // ─── Installs keyed by slug + app_version_id ────────────────────────────

    /// Uninstall a plugin from an app version: `DELETE .../{slug}/installs/{app_version_id}`.
    pub async fn uninstall(&self, slug: &str, app_version_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &install_path(slug, app_version_id, ""));
        self.client.execute(builder).await
    }

    /// Enable a plugin install: `POST .../{slug}/installs/{app_version_id}/enable`.
    pub async fn enable(&self, slug: &str, app_version_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &install_path(slug, app_version_id, "/enable"));
        self.client.execute(builder).await
    }

    /// Disable a plugin install: `POST .../{slug}/installs/{app_version_id}/disable`.
    pub async fn disable(&self, slug: &str, app_version_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &install_path(slug, app_version_id, "/disable"));
        self.client.execute(builder).await
    }

    /// Get install settings: `GET .../{slug}/installs/{app_version_id}/settings`.
    pub async fn get_settings(&self, slug: &str, app_version_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_path(slug, app_version_id, "/settings"));
        self.client.execute(builder).await
    }

    /// Update install settings: `PATCH .../{slug}/installs/{app_version_id}/settings`.
    pub async fn update_settings(&self, slug: &str, app_version_id: i64, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PATCH, &install_path(slug, app_version_id, "/settings"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Get system settings: `GET .../{slug}/installs/{app_version_id}/settings/system`.
    pub async fn get_system_settings(&self, slug: &str, app_version_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_path(slug, app_version_id, "/settings/system"));
        self.client.execute(builder).await
    }

    /// Update system settings: `PATCH .../{slug}/installs/{app_version_id}/settings/system`.
    pub async fn update_system_settings(&self, slug: &str, app_version_id: i64, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PATCH, &install_path(slug, app_version_id, "/settings/system"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Get install scopes: `GET .../{slug}/installs/{app_version_id}/scopes`.
    pub async fn get_scopes(&self, slug: &str, app_version_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_path(slug, app_version_id, "/scopes"));
        self.client.execute(builder).await
    }

    /// Set install scopes: `POST .../{slug}/installs/{app_version_id}/scopes` with `{scopes}`.
    pub async fn update_scopes(&self, slug: &str, app_version_id: i64, scopes: &[String]) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &install_path(slug, app_version_id, "/scopes"))
            .json(&serde_json::json!({ "scopes": scopes }));
        self.client.execute(builder).await
    }

    // ─── Installs keyed by numeric install id ───────────────────────────────

    /// Uninstall by install id: `DELETE /api/platform/plugins/installs/{install_id}`.
    pub async fn uninstall_by_id(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &install_id_path(install_id, ""));
        self.client.execute(builder).await
    }

    /// Enable by install id: `POST /api/platform/plugins/installs/{install_id}/enable`.
    pub async fn enable_by_id(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &install_id_path(install_id, "/enable"));
        self.client.execute(builder).await
    }

    /// Disable by install id: `POST /api/platform/plugins/installs/{install_id}/disable`.
    pub async fn disable_by_id(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &install_id_path(install_id, "/disable"));
        self.client.execute(builder).await
    }

    /// Get settings by install id: `GET /api/platform/plugins/installs/{install_id}/settings`.
    pub async fn get_settings_by_id(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_id_path(install_id, "/settings"));
        self.client.execute(builder).await
    }

    /// Update settings by install id: `PATCH /api/platform/plugins/installs/{install_id}/settings`.
    pub async fn update_settings_by_id(&self, install_id: i64, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PATCH, &install_id_path(install_id, "/settings"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Get scopes by install id: `GET /api/platform/plugins/installs/{install_id}/scopes`.
    pub async fn get_scopes_by_id(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_id_path(install_id, "/scopes"));
        self.client.execute(builder).await
    }

    /// Set scopes by install id: `POST /api/platform/plugins/installs/{install_id}/scopes`.
    pub async fn update_scopes_by_id(&self, install_id: i64, scopes: &[String]) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &install_id_path(install_id, "/scopes"))
            .json(&serde_json::json!({ "scopes": scopes }));
        self.client.execute(builder).await
    }

    // ─── Runtime / instances (keyed by install id) ──────────────────────────

    /// Runtime info: `GET /api/platform/plugins/installs/{install_id}/runtime`.
    pub async fn runtime_info(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_id_path(install_id, "/runtime"));
        self.client.execute(builder).await
    }

    /// List instances: `GET /api/platform/plugins/installs/{install_id}/instances`.
    pub async fn instances(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_id_path(install_id, "/instances"));
        self.client.execute(builder).await
    }

    /// Instance detail: `GET .../installs/{install_id}/instances/{task_id}`.
    pub async fn instance_detail(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_id_path(install_id, &format!("/instances/{task_id}")));
        self.client.execute(builder).await
    }

    /// Instance stats: `GET .../installs/{install_id}/instances/{task_id}/stats`.
    pub async fn instance_stats(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_id_path(install_id, &format!("/instances/{task_id}/stats")));
        self.client.execute(builder).await
    }

    /// Instance logs: `GET .../installs/{install_id}/instances/{task_id}/logs`.
    pub async fn instance_logs(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_id_path(install_id, &format!("/instances/{task_id}/logs")));
        self.client.execute(builder).await
    }

    /// Versions installed for an install id: `GET /api/platform/plugins/installs/{install_id}/versions`.
    pub async fn install_versions(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &install_id_path(install_id, "/versions"));
        self.client.execute(builder).await
    }

    // ─── Slug-keyed info surfaces ───────────────────────────────────────────

    /// Available versions of a plugin: `GET /api/platform/plugins/{slug}/versions`.
    pub async fn versions(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &slug_path(slug, "/versions"));
        self.client.execute(builder).await
    }

    /// Request logs: `GET /api/platform/plugins/{slug}/logs`.
    pub async fn request_logs(&self, slug: &str, params: LogListParams) -> Result<Value, AlcedoError> {
        let mut builder = self.client.request(Method::GET, &slug_path(slug, "/logs"));
        let query = params.to_query();
        if !query.is_empty() {
            builder = builder.query(&query);
        }
        self.client.execute(builder).await
    }

    /// Request log detail: `GET /api/platform/plugins/{slug}/logs/{request_id}`.
    pub async fn request_log_detail(&self, slug: &str, request_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &slug_path(slug, &format!("/logs/{request_id}")));
        self.client.execute(builder).await
    }

    /// List docs: `GET /api/platform/plugins/{slug}/docs`.
    pub async fn docs(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &slug_path(slug, "/docs"));
        self.client.execute(builder).await
    }

    /// Get doc content: `GET /api/platform/plugins/{slug}/docs/{path}`.
    pub async fn doc_content(&self, slug: &str, doc_path: &str) -> Result<String, AlcedoError> {
        let clean = doc_path
            .strip_prefix("docs/")
            .or_else(|| doc_path.strip_prefix("./"))
            .unwrap_or(doc_path);
        let builder = self
            .client
            .request(Method::GET, &slug_path(slug, &format!("/docs/{clean}")));
        let resp: serde_json::Value = self.client.execute(builder).await?;
        Ok(match resp {
            Value::String(s) => s,
            other => other.to_string(),
        })
    }

    /// Plugin DB schema: `GET /api/platform/plugins/{slug}/schema`.
    pub async fn schema(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &slug_path(slug, "/schema"));
        self.client.execute(builder).await
    }

    /// Event subscriptions: `GET /api/platform/plugins/{slug}/events`.
    pub async fn events(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &slug_path(slug, "/events"));
        self.client.execute(builder).await
    }

    /// Plugin pages: `GET /api/platform/plugins/{slug}/pages`.
    pub async fn pages(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &slug_path(slug, "/pages"));
        self.client.execute(builder).await
    }

    /// Plugin page assets: `GET /api/platform/plugins/{slug}/pages/assets`.
    pub async fn assets(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &slug_path(slug, "/pages/assets"));
        self.client.execute(builder).await
    }

    // ─── Lifecycle ──────────────────────────────────────────────────────────

    /// Scale a plugin: `POST /api/platform/plugins/{slug}/scale`.
    pub async fn scale(&self, slug: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &slug_path(slug, "/scale"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Restart a plugin: `POST /api/platform/plugins/{slug}/restart` (+ optional `{deployment_id}`).
    pub async fn restart(&self, slug: &str, deployment_id: Option<&str>) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &slug_path(slug, "/restart"))
            .json(&serde_json::json!({ "deployment_id": deployment_id }));
        self.client.execute(builder).await
    }

    /// Stop a plugin: `POST /api/platform/plugins/{slug}/stop`.
    pub async fn stop(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::POST, &slug_path(slug, "/stop"));
        self.client.execute(builder).await
    }
}
