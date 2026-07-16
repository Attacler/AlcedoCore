use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: u32,
    pub truncated: bool,
    pub execution_time_ms: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub core: HealthCore,
    pub plugins: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthCore {
    pub db: String,
    pub docker: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsResponse {
    pub settings: Value,
    #[serde(default)]
    pub schema: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MigrationStatus {
    pub version: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub applied_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KvPair {
    pub key: String,
    pub value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub action: String,
    pub target: String,
    pub created_at: String,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginSchemaResponse {
    pub plugin_name: String,
    pub schema_name: String,
    pub tables: Vec<TableDefinition>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableDefinition {
    pub name: String,
    pub columns: Vec<ColumnDefinition>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    #[serde(default)]
    pub is_primary_key: bool,
    #[serde(default)]
    pub foreign_key: Option<ForeignKeyInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ForeignKeyInfo {
    pub referenced_table: String,
    pub referenced_column: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DevSessionResponse {
    pub slug: String,
    pub url: String,
    pub expires_at: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KvBatchGetResponse {
    #[serde(default)]
    pub values: HashMap<String, Option<Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KvBatchDeleteResponse {
    pub deleted: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KvExistsResponse {
    pub exists: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KvTtlResponse {
    #[serde(default)]
    pub ttl: Option<i64>,
}
