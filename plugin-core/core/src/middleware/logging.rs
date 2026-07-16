use tokio::sync::mpsc;

use crate::db::Pool;
use crate::db::queries::RequestLog;
use chrono::Utc;

use serde_json;

#[derive(Clone)]
pub struct LoggingChannel {
    tx: mpsc::Sender<LogEntry>,
}

pub struct LogEntry {
    pub request_id: String,
    pub plugin_slug: String,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub duration_ms: i64,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_body: Option<String>,
    pub request_headers: Option<serde_json::Value>,
    pub request_body_size: Option<i32>,
    pub timestamp: chrono::DateTime<Utc>,
}

impl LoggingChannel {
    pub fn new(tx: mpsc::Sender<LogEntry>) -> Self {
        LoggingChannel { tx }
    }

    pub async fn log(&self, entry: LogEntry) {
        if let Err(e) = self.tx.send(entry).await {
            tracing::error!("Failed to send log entry: {:?}", e);
        }
    }
}

pub fn spawn_log_writer(pool: Pool) -> LoggingChannel {
    let (tx, mut rx) = mpsc::channel::<LogEntry>(1000);

    tokio::spawn(async move {
        tracing::info!("[LOG_WRITER] Started log writer task");
        while let Some(entry) = rx.recv().await {
            tracing::debug!("[LOG_WRITER] Received log entry for {}", entry.plugin_slug);
            let log = RequestLog {
                id: 0,
                request_id: entry.request_id.clone(),
                plugin_slug: entry.plugin_slug.clone(),
                timestamp: entry.timestamp,
                method: entry.method.clone(),
                path: entry.path.clone(),
                status_code: entry.status_code,
                duration_ms: entry.duration_ms as i32,
                client_ip: entry.client_ip.clone(),
                user_agent: entry.user_agent.clone(),
                source: "production".to_string(),
                request_body: entry.request_body.clone(),
                request_headers: entry.request_headers.clone(),
                request_body_size: entry.request_body_size,
                created_at: Utc::now(),
            };

            if let Err(e) = RequestLog::insert(&pool, &log).await {
                tracing::warn!(request_id = %entry.request_id, "Failed to insert request log (slug likely missing): {}", e);
            } else {
                tracing::debug!("[LOG_WRITER] Inserted log for {}", entry.plugin_slug);
            }
        }
        tracing::info!("[LOG_WRITER] Task ending");
    });

    LoggingChannel { tx }
}

pub async fn log_request(
    logging_channel: &LoggingChannel,
    request_id: String,
    plugin_slug: String,
    method: String,
    path: String,
    status_code: i32,
    duration_ms: i64,
    client_ip: Option<String>,
    user_agent: Option<String>,
    request_body: Option<String>,
    request_headers: Option<serde_json::Value>,
    request_body_size: Option<i32>,
) {
    let entry = LogEntry {
        request_id,
        plugin_slug,
        method,
        path,
        status_code,
        duration_ms,
        client_ip,
        user_agent,
        request_body,
        request_headers,
        request_body_size,
        timestamp: Utc::now(),
    };

    let _ = logging_channel.log(entry).await;
}

pub fn extract_request_id_from_headers(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

pub fn extract_client_ip_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
}