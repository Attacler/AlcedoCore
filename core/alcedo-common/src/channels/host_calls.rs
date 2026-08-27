use tokio::sync::mpsc;
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