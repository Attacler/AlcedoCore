use crate::error::AlcedoError;
use reqwest::{Client, RequestBuilder, Response};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct BaseClient {
    client: Client,
    base_url: String,
    plugin_slug: String,
    request_id: Option<String>,
    api_key: Option<String>,
    app: Option<String>,
    version: Option<String>,
}

impl BaseClient {
    pub fn new(
        base_url: &str,
        plugin_slug: &str,
        timeout: Duration,
        request_id: Option<String>,
    ) -> Result<Self, AlcedoError> {
        Self::with_context(base_url, plugin_slug, timeout, request_id, None, None, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_context(
        base_url: &str,
        plugin_slug: &str,
        timeout: Duration,
        request_id: Option<String>,
        api_key: Option<String>,
        app: Option<String>,
        version: Option<String>,
    ) -> Result<Self, AlcedoError> {
        let client = Client::builder()
            .timeout(timeout)
            .pool_max_idle_per_host(5)
            .build()
            .map_err(|e| AlcedoError::Connection {
                message: format!("Failed to build HTTP client: {}", e),
                status_code: None,
                source: Some(Box::new(e)),
            })?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            plugin_slug: plugin_slug.to_string(),
            request_id,
            api_key,
            app,
            version,
        })
    }

    fn apply_headers(&self, builder: RequestBuilder, app: Option<&str>, version: Option<&str>) -> RequestBuilder {
        let builder = builder.header(
            "X-Request-ID",
            self.request_id
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
        );
        let builder = if let Some(key) = &self.api_key {
            builder.header("Authorization", format!("Bearer {}", key))
        } else {
            builder
        };
        let app = app.or(self.app.as_deref());
        let version = version.or(self.version.as_deref());
        let builder = if let Some(app) = app {
            builder.header("X-App", app)
        } else {
            builder
        };
        if let Some(version) = version {
            builder.header("X-Version", version)
        } else {
            builder
        }
    }

    pub fn request(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        let url = format!(
            "{}/{}",
            self.base_url,
            path.trim_start_matches('/')
        );
        let builder = self.client.request(method, &url);
        self.apply_headers(builder, None, None)
    }

    /// Like [`request`](Self::request), but with per-call `X-App` / `X-Version`
    /// overrides. `Some` replaces the client-level default, `None` keeps it.
    pub fn request_with_context(
        &self,
        method: reqwest::Method,
        path: &str,
        app: Option<&str>,
        version: Option<&str>,
    ) -> RequestBuilder {
        let url = format!(
            "{}/{}",
            self.base_url,
            path.trim_start_matches('/')
        );
        let builder = self.client.request(method, &url);
        self.apply_headers(builder, app, version)
    }

    pub async fn send(&self, builder: RequestBuilder) -> Result<Response, AlcedoError> {
        builder.send().await.map_err(|e| AlcedoError::Connection {
            message: format!("HTTP request failed: {}", e),
            status_code: e.status().map(|s| s.as_u16()),
            source: Some(Box::new(e)),
        })
    }

    pub async fn execute_json<T: serde::de::DeserializeOwned>(
        &self,
        builder: RequestBuilder,
    ) -> Result<T, AlcedoError> {
        let response = self.send(builder).await?;
        let status = response.status().as_u16();
        let text = response.text().await.map_err(|e| AlcedoError::Connection {
            message: format!("Failed to read response body: {}", e),
            status_code: Some(status),
            source: Some(Box::new(e)),
        })?;

        let body: Option<Value> = serde_json::from_str(&text).ok();

        // JSend envelope: {status:'success', data} unwraps to data;
        // {status:'fail'|'error', message, code} raises an Api error.
        if let Some(obj) = body.as_ref().and_then(|b| b.as_object()) {
            if let Some(jsend_status) = obj.get("status").and_then(|s| s.as_str()) {
                match jsend_status {
                    "success" => {
                        let data = obj.get("data").cloned().unwrap_or(Value::Null);
                        return serde_json::from_value(data).map_err(|e| AlcedoError::Connection {
                            message: format!("Failed to deserialize response data: {}", e),
                            status_code: Some(status),
                            source: Some(Box::new(e)),
                        });
                    }
                    "fail" | "error" => {
                        let message = obj
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or(&text)
                            .to_string();
                        let code = obj.get("code").map(|c| {
                            c.as_str()
                                .map(String::from)
                                .unwrap_or_else(|| c.to_string())
                        });
                        return Err(AlcedoError::Api {
                            message,
                            code,
                            status: jsend_status.to_string(),
                            http_status: Some(status),
                        });
                    }
                    _ => {}
                }
            }
        }

        if !response_status_is_success(status) {
            let message = body
                .as_ref()
                .and_then(|b| b.get("error").and_then(|e| e.get("message")).or_else(|| b.get("message")).and_then(|m| m.as_str()).map(String::from))
                .unwrap_or_else(|| text.clone());
            return Err(AlcedoError::from_status(status, message));
        }

        // Non-JSend body (e.g. POST /api/dev/request-id's bare {request_id}) passes through.
        match body {
            Some(value) => serde_json::from_value(value).map_err(|e| AlcedoError::Connection {
                message: format!("Failed to deserialize response: {}", e),
                status_code: Some(status),
                source: Some(Box::new(e)),
            }),
            None => serde_json::from_str(&text).map_err(|e| AlcedoError::Connection {
                message: format!("Failed to deserialize response: {}", e),
                status_code: Some(status),
                source: Some(Box::new(e)),
            }),
        }
    }

    pub async fn execute(&self, builder: RequestBuilder) -> Result<serde_json::Value, AlcedoError> {
        self.execute_json(builder).await
    }

    pub fn plugin_slug(&self) -> &str {
        &self.plugin_slug
    }

    pub fn app(&self) -> Option<&str> {
        self.app.as_deref()
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

fn response_status_is_success(status: u16) -> bool {
    (200..300).contains(&status)
}
