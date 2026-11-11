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

// ─── Auth / users / sessions ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FetchMeResponse {
    pub user: User,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub user: User,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub user_id: String,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub current: bool,
}

// ─── Apps / versions ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AppWithVersions {
    pub id: i64,
    pub name: String,
    pub api_name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub versions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserAppAccess {
    pub app_id: i64,
    pub app_name: String,
    pub api_name: String,
    pub version: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppVersionRow {
    pub id: i64,
    pub app_id: i64,
    pub app_name: String,
    pub api_name: String,
    pub version_id: i64,
    pub version_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Version {
    pub id: i64,
    pub version_name: String,
}

// ─── Settings ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformSettings {
    pub platform_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsUpdateResponse {
    #[serde(default)]
    pub description: Option<String>,
    pub key: String,
    pub updated_at: String,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsBatchUpdateResponse {
    pub count: u32,
    pub success: bool,
}

// ─── Route mappings / developer keys ────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct RouteMapping {
    pub id: i64,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub path_prefix: Option<String>,
    pub install_id: i64,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub install_slug: Option<String>,
    #[serde(default)]
    pub install_app: Option<String>,
    #[serde(default)]
    pub install_version: Option<String>,
    #[serde(default)]
    pub effective_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeveloperKey {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version_id: Option<i64>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeveloperKeyWithRawKey {
    #[serde(flatten)]
    pub key: DeveloperKey,
    pub raw_key: String,
}

// ─── Menus ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub external: Option<bool>,
    #[serde(default)]
    pub link_type: Option<String>,
    #[serde(default)]
    pub children: Vec<MenuItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MenuSection {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Menu {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub sections: Vec<MenuSection>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MenuListEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub role_count: i64,
    #[serde(default)]
    pub item_count: i64,
    #[serde(default)]
    pub created_at: String,
}

// ─── Activity logs ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub actor_id: Option<String>,
    pub collection_name: String,
    pub item_id: String,
    pub request_id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub diff: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimelineEntrySystem {
    pub action: String,
    #[serde(default)]
    pub actor_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub description: String,
    pub id: i64,
    #[serde(default)]
    pub metadata: Option<Value>,
    pub request_id: String,
    pub target: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActivityLogPage<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

// ─── Collections / lookup ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionField {
    #[serde(default)]
    pub default: Option<Value>,
    pub display_name: String,
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub is_system: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Collection {
    pub created_at: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub fields: Vec<CollectionField>,
    #[serde(default)]
    pub is_system: bool,
    pub name: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionListResponse {
    pub collections: Vec<Collection>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserOption {
    pub id: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

// ─── Files ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub alt_text: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileListResponse {
    pub data: Vec<MediaFile>,
    #[serde(default)]
    pub total: i64,
}

// ─── Dev ────────────────────────────────────────────────────────────────────

/// Bare (non-JSend) response of `POST /api/dev/request-id`.
#[derive(Debug, Serialize, Deserialize)]
pub struct RequestIdResponse {
    pub request_id: String,
}
