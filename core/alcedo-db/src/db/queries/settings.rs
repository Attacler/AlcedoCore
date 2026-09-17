use crate::find_by;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct SystemSetting {
    pub key: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl SystemSetting {
    find_by!(
        find_by_key,
        "alcedocore_system_settings",
        "key, value, description, updated_at",
        "key"
    );

}
