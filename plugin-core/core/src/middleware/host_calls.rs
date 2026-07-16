use tokio::sync::mpsc;
use crate::db::Pool;
use crate::db::queries::HostCallLog;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    KvGet,
    KvPut,
    KvDelete,
    KvExists,
    KvTtl,
    KvList,
    KvBatchGet,
    KvBatchSet,
    KvBatchDelete,
    KvIncrement,
    KvDecrement,
    DbQuery,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionType::KvGet => write!(f, "kv_get"),
            ActionType::KvPut => write!(f, "kv_put"),
            ActionType::KvDelete => write!(f, "kv_delete"),
            ActionType::KvExists => write!(f, "kv_exists"),
            ActionType::KvTtl => write!(f, "kv_ttl"),
            ActionType::KvList => write!(f, "kv_list"),
            ActionType::KvBatchGet => write!(f, "kv_batch_get"),
            ActionType::KvBatchSet => write!(f, "kv_batch_set"),
            ActionType::KvBatchDelete => write!(f, "kv_batch_delete"),
            ActionType::KvIncrement => write!(f, "kv_increment"),
            ActionType::KvDecrement => write!(f, "kv_decrement"),
            ActionType::DbQuery => write!(f, "db_query"),
        }
    }
}

#[derive(Clone)]
pub struct HostCallChannel {
    tx: mpsc::Sender<HostCallEntry>,
}

#[derive(Debug)]
pub struct HostCallEntry {
    pub request_id: String,
    pub action_type: ActionType,
    pub args_summary: String,
    pub result_summary: String,
    pub duration_ms: i32,
}

impl HostCallChannel {
    pub fn new(tx: mpsc::Sender<HostCallEntry>) -> Self {
        HostCallChannel { tx }
    }

    /// Fire-and-forget log: uses try_send so it never blocks the caller.
    /// Drops the entry with a warning if the channel is full.
    pub fn try_log(&self, entry: HostCallEntry) {
        if let Err(e) = self.tx.try_send(entry) {
            tracing::warn!("[HOST_CALL] Channel full, dropping entry: {:?}", e);
        }
    }
}

pub fn spawn_host_call_writer(pool: Pool) -> HostCallChannel {
    let (tx, mut rx) = mpsc::channel::<HostCallEntry>(10_000);

    tokio::spawn(async move {
        tracing::info!("[HOST_CALL_WRITER] Started host call writer task");
        while let Some(entry) = rx.recv().await {
            let log = HostCallLog {
                id: 0,
                parent_request_id: entry.request_id,
                action_type: entry.action_type.to_string(),
                args_summary: entry.args_summary,
                result_summary: entry.result_summary,
                duration_ms: entry.duration_ms,
                created_at: chrono::Utc::now(),
            };
            if let Err(e) = HostCallLog::insert(&pool, &log).await {
                tracing::error!(request_id = %log.parent_request_id, "Failed to insert host call: {}", e);
            }
        }
        tracing::info!("[HOST_CALL_WRITER] Task ending");
    });

    HostCallChannel { tx }
}

/// Records a host call entry via the channel.
///
/// If `request_id` is None, generates a UUID v4 fallback (REQID-03).
/// This is a synchronous fire-and-forget operation — never blocks on channel capacity.
pub fn record_host_call(
    channel: &HostCallChannel,
    request_id: Option<String>,
    action_type: ActionType,
    args_summary: String,
    result_summary: String,
    duration_ms: i32,
) {
    let request_id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let entry = HostCallEntry {
        request_id,
        action_type,
        args_summary,
        result_summary,
        duration_ms,
    };
    channel.try_log(entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_send_receive() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (tx, mut rx) = mpsc::channel::<HostCallEntry>(10);
            let channel = HostCallChannel::new(tx);

            let entry = HostCallEntry {
                request_id: "test-id".to_string(),
                action_type: ActionType::KvGet,
                args_summary: "key: test-key".to_string(),
                result_summary: "value: test-val".to_string(),
                duration_ms: 42,
            };

            channel.try_log(entry);
            let received = rx.recv().await.unwrap();

            assert_eq!(received.request_id, "test-id");
            assert_eq!(received.action_type.to_string(), "kv_get");
            assert_eq!(received.args_summary, "key: test-key");
            assert_eq!(received.duration_ms, 42);
        });
    }

    #[test]
    fn test_channel_full_does_not_panic() {
        let (tx, _rx) = mpsc::channel::<HostCallEntry>(1);
        let channel = HostCallChannel::new(tx);

        let entry = HostCallEntry {
            request_id: "first".to_string(),
            action_type: ActionType::KvGet,
            args_summary: String::new(),
            result_summary: String::new(),
            duration_ms: 0,
        };
        channel.try_log(entry);

        // This send should fail silently (warning log) — no panic
        let entry2 = HostCallEntry {
            request_id: "second".to_string(),
            action_type: ActionType::KvGet,
            args_summary: String::new(),
            result_summary: String::new(),
            duration_ms: 0,
        };
        channel.try_log(entry2);
        // If we got here without panic, test passes
    }

    #[test]
    fn test_record_host_call_generates_uuid_when_no_request_id() {
        let (tx, mut rx) = mpsc::channel::<HostCallEntry>(10);
        let channel = HostCallChannel::new(tx);

        record_host_call(
            &channel,
            None,
            ActionType::DbQuery,
            "SELECT * FROM test".to_string(),
            "rows: 5".to_string(),
            100,
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let received = rx.recv().await.unwrap();
            assert_eq!(received.request_id.len(), 36);
            assert!(uuid::Uuid::parse_str(&received.request_id).is_ok());
            assert_eq!(received.action_type.to_string(), "db_query");
        });
    }

        #[test]
        fn test_action_type_display() {
            assert_eq!(ActionType::KvGet.to_string(), "kv_get");
            assert_eq!(ActionType::KvPut.to_string(), "kv_put");
            assert_eq!(ActionType::KvDelete.to_string(), "kv_delete");
            assert_eq!(ActionType::KvExists.to_string(), "kv_exists");
            assert_eq!(ActionType::KvTtl.to_string(), "kv_ttl");
            assert_eq!(ActionType::KvList.to_string(), "kv_list");
            assert_eq!(ActionType::KvBatchGet.to_string(), "kv_batch_get");
            assert_eq!(ActionType::KvBatchSet.to_string(), "kv_batch_set");
            assert_eq!(ActionType::KvBatchDelete.to_string(), "kv_batch_delete");
            assert_eq!(ActionType::DbQuery.to_string(), "db_query");
        }

        #[test]
        fn test_action_type_serde_snake_case() {
            let json = serde_json::to_value(ActionType::KvGet).unwrap();
            assert_eq!(json, serde_json::json!("kv_get"));

            let deserialized: ActionType = serde_json::from_str("\"kv_get\"").unwrap();
            assert!(matches!(deserialized, ActionType::KvGet));

            let deserialized: ActionType = serde_json::from_str("\"kv_exists\"").unwrap();
            assert!(matches!(deserialized, ActionType::KvExists));

            let deserialized: ActionType = serde_json::from_str("\"db_query\"").unwrap();
            assert!(matches!(deserialized, ActionType::DbQuery));
        }
}
