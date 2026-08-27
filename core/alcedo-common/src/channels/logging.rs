use tokio::sync::mpsc;

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