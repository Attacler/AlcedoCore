use crate::db::fields;
use crate::db::Pool;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// The supported field types per COLL-07, plus Relationship (Phase 27) and Boolean
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    #[serde(rename = "string")]
    String,
    Text,
    #[serde(rename = "int")]
    Int,
    Float,
    Datetime,
    Uuid,
    Relationship,
    #[serde(rename = "boolean")]
    Bool,
    /// File field type — stores UUID[] of file_metadata IDs.
    /// Resolved to full file metadata objects in API responses.
    #[serde(rename = "file")]
    File,
}

/// A single field in a collection definition per COLL-08
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]

pub struct FieldDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Maps to the display/input widget type (e.g. "single-line", "pick-list")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_type: Option<String>,
    /// Explicit input widget used when editing this field (e.g. "email",
    /// "number", "file"). When unset, the input is resolved from the field type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_component: Option<String>,
    /// Explicit display widget used when rendering this field read-only
    /// (e.g. "email", "currency", "file-list"). When unset, the display is
    /// resolved from the field type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_component: Option<String>,
    /// For Relationship fields: the referenced collection name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_collection: Option<String>,
    /// For Relationship fields: "one_to_one", "many_to_one", or "one_to_many"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_type: Option<String>,
    /// For Relationship fields: the field on the related collection to use as display label
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_field: Option<String>,
    /// For Relationship fields: list of parent fields to show inline on child record detail
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_parent_fields: Option<Vec<String>>,
    /// Field-specific options. For pick-list/multi-select this is an array of
    /// label/value options; for file fields it is an object (e.g. multiple,
    /// max_file_size, allowed_mime_types).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,

    #[serde(default)]
    pub is_system: bool,

    #[serde(default)]
    pub hidden: bool,
}

/// Full collection definition as stored in collection_definitions table
#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct CollectionDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub fields: Vec<FieldDefinition>,
    #[serde(default)]
    pub is_system: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_slug: Option<String>,

    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// A group of items that reference a given item through a relationship field.
#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct ReferencingGroup {
    /// The collection that contains the reference
    pub collection_name: String,
    /// The field in that collection that holds the FK
    pub field_name: String,
    /// The relationship type ("one_to_one" or "many_to_one")
    pub relationship_type: String,
    /// The items that reference the target item
    pub items: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]

pub struct UpdateCollectionMetaDataResponse {
    success: bool,
}

/// POST /api/collections request body
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]

pub struct CreateCollectionRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub fields: Vec<FieldDefinition>,
}

/// PUT /api/collections/:name request body
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]

pub struct UpdateCollectionRequest {
    pub fields: Vec<FieldDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

const RESERVED_TABLE_NAMES: &[&str] = &[
    "plugins",
    "plugin_versions",
    "registries",
    "request_logs",
    "schema_migrations",
    "collection_definitions",
    "_sqlx_migrations",
];

/// Validate collection name per COLL-02:
/// - Must match ^[a-z][a-z0-9_]*$ (starts with lowercase letter)
/// - Max 59 characters (PostgreSQL identifier limit - NAMEDATALEN/2 margin)
/// - No reserved system table names
pub fn validate_collection_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "Collection name cannot be empty".to_string(),
        ));
    }
    if name.len() > 59 {
        return Err(AppError::BadRequest(format!(
            "Collection name too long: {} chars (max 59)",
            name.len()
        )));
    }
    let re = regex::Regex::new(r"^[a-z][a-z0-9_]*$").unwrap();
    if !re.is_match(name) {
        return Err(AppError::BadRequest(format!(
            "Invalid collection name '{}': must match ^[a-z][a-z0-9_]*$ (lowercase letter followed by lowercase letters, digits, underscores)",
            name
        )));
    }
    if RESERVED_TABLE_NAMES.contains(&name) {
        return Err(AppError::BadRequest(format!(
            "Collection name '{}' is a reserved system table name",
            name
        )));
    }
    Ok(())
}

/// Validate field names: alphanumeric + underscore, starts with letter, no reserved names
pub fn validate_field_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "Field name cannot be empty".to_string(),
        ));
    }
    if name.len() > 59 {
        return Err(AppError::BadRequest(format!(
            "Field name too long: {} chars (max 59)",
            name.len()
        )));
    }
    let re = regex::Regex::new(r"^[a-zA-Z][a-zA-Z0-9_]*$").unwrap();
    if !re.is_match(name) {
        return Err(AppError::BadRequest(format!(
            "Invalid field name '{}': must start with letter and contain only alphanumeric + underscores",
            name
        )));
    }
    // Reserved field names that conflict with implicit columns
    let reserved_fields = ["id", "created_at", "updated_at"];
    if reserved_fields.contains(&name) {
        return Err(AppError::BadRequest(format!(
            "Field name '{}' is reserved for implicit system columns",
            name
        )));
    }
    Ok(())
}

/// Validate all fields in a collection creation/update request
pub fn validate_fields(fields: &[FieldDefinition]) -> Result<(), AppError> {
    if fields.is_empty() {
        return Err(AppError::BadRequest(
            "Collection must have at least one field".to_string(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        validate_field_name(&field.name)?;
        if !seen.insert(field.name.clone()) {
            return Err(AppError::BadRequest(format!(
                "Duplicate field name: '{}'",
                field.name
            )));
        }

        // Validate relationship fields (Phase 27)
        if field.field_type == FieldType::Relationship {
            if field.related_collection.is_none()
                || field
                    .related_collection
                    .as_ref()
                    .map_or(true, |c| c.is_empty())
            {
                return Err(AppError::BadRequest(format!(
                    "Relationship field '{}' must have a related_collection specified",
                    field.name
                )));
            }
            match field.relationship_type.as_deref() {
                Some("one_to_one") | Some("many_to_one") | Some("one_to_many") => {}
                Some(other) => {
                    return Err(AppError::BadRequest(format!(
                        "Invalid relationship_type '{}' for field '{}'. Must be 'one_to_one', 'many_to_one', or 'one_to_many'",
                        other, field.name
                    )))
                }
                None => {
                    return Err(AppError::BadRequest(format!(
                        "Relationship field '{}' must have a relationship_type specified",
                        field.name
                    )))
                }
            }
            // Validate related_collection name format
            if let Some(ref rc) = field.related_collection {
                validate_collection_name(rc)?;
            }
        }
    }
    Ok(())
}

/// List collections accessible by a user via their role policies.
/// Admin users (users.all) get all collections. Unauthenticated callers get empty list.
pub async fn list_accessible_collections(
    pool: &Pool,
    user_id: Option<&Uuid>,
    is_admin: bool,
) -> Result<Vec<CollectionDefinition>, AppError> {
    if is_admin {
        return list_collections(pool).await;
    }

    let Some(user_id) = user_id else {
        return Ok(vec![]);
    };

    let rows = sqlx::query_as::<_, (String, Option<String>, bool, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        r#"SELECT DISTINCT cd.name, cd.display_name, cd.is_system, cd.plugin_slug, cd.created_at, cd.updated_at
           FROM collection_definitions cd
           JOIN policy_permissions pp ON pp.collection_name = cd.name
           JOIN role_policies rp ON rp.policy_id = pp.policy_id
           JOIN user_roles ur ON ur.role_id = rp.role_id
           WHERE ur.user_id = $1
           ORDER BY cd.is_system ASC, cd.name ASC"#
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to list accessible collections: {}", e) })?;

    let mut collections = Vec::new();
    for (name, display_name, is_system, plugin_slug, created_at, updated_at) in rows {
        let field_rows = fields::list_fields(pool, &name).await?;
        let fields: Vec<FieldDefinition> = field_rows.iter().map(|r| r.to_definition()).collect();
        collections.push(CollectionDefinition {
            name,
            display_name,
            fields,
            is_system,
            plugin_slug,
            created_at: Some(created_at),
            updated_at: Some(updated_at),
        });
    }
    Ok(collections)
}

/// List all collection definitions
pub async fn list_collections(pool: &Pool) -> Result<Vec<CollectionDefinition>, AppError> {
    let rows = sqlx::query_as::<_, (String, Option<String>,  bool, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT name, display_name, is_system, plugin_slug, created_at, updated_at FROM collection_definitions ORDER BY updated_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to list collections: {}", e) })?;

    let mut collections = Vec::new();
    for (name, display_name, is_system, plugin_slug, created_at, updated_at) in rows {
        let field_rows = fields::list_fields(pool, &name).await?;
        let fields: Vec<FieldDefinition> = field_rows.iter().map(|r| r.to_definition()).collect();
        collections.push(CollectionDefinition {
            name,
            display_name,
            fields,
            is_system,
            plugin_slug,
            created_at: Some(created_at),
            updated_at: Some(updated_at),
        });
    }
    Ok(collections)
}

/// Get a single collection definition by name, within a transaction
pub async fn get_collection_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    name: &str,
) -> Result<CollectionDefinition, AppError> {
    let row = sqlx::query_as::<_, (String, Option<String>, bool, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT name, display_name, is_system, plugin_slug, created_at, updated_at FROM collection_definitions WHERE name = $1"
    )
    .bind(name)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to get collection: {}", e) })?
    .ok_or_else(|| AppError::NotFound(format!("Collection '{}' not found", name)))?;

    let field_rows = fields::list_fields_in_tx(tx, name).await?;
    let fields: Vec<FieldDefinition> = field_rows.iter().map(|r| r.to_definition()).collect();

    Ok(CollectionDefinition {
        name: row.0,
        display_name: row.1,
        fields,
        is_system: row.2,
        plugin_slug: row.3,
        created_at: Some(row.4),
        updated_at: Some(row.5),
    })
}

/// Get a single collection definition by name (pool-based)
pub async fn get_collection(pool: &Pool, name: &str) -> Result<CollectionDefinition, AppError> {
    let row = sqlx::query_as::<_, (String, Option<String>, bool, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT name, display_name, is_system, plugin_slug, created_at, updated_at FROM collection_definitions WHERE name = $1"
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to get collection: {}", e) })?
    .ok_or_else(|| AppError::NotFound(format!("Collection '{}' not found", name)))?;

    let field_rows = fields::list_fields(pool, name).await?;
    let fields: Vec<FieldDefinition> = field_rows.iter().map(|r| r.to_definition()).collect();

    Ok(CollectionDefinition {
        name: row.0,
        display_name: row.1,
        fields,
        is_system: row.2,
        plugin_slug: row.3,
        created_at: Some(row.4),
        updated_at: Some(row.5),
    })
}

/// Get all items that reference a specific item across all collections.
///
/// 1. Lists all collection definitions
/// 2. Filters for collections that have relationship fields pointing to `collection_name`
/// 3. For each referencing collection+field pair, queries items where FK = `item_id`
/// 4. Returns results grouped per (referencing_collection, field_name)
///
/// Uses raw SQL with parameterized binds (no string interpolation for values).
/// The table and column names are safely quoted via quote_id() since they
/// come from trusted collection metadata, not user input.
pub async fn get_referencing_items(
    pool: &Pool,
    collection_name: &str,
    item_id: &str,
    collection_filters: &HashMap<String, Vec<crate::services::permissions::PolicyPermission>>,
) -> Result<Vec<ReferencingGroup>, AppError> {
    // Helper to quote identifiers with sea-query
    fn quote_id(name: &str) -> String {
        crate::db::quote_identifier(name)
    }

    // 1. List all collections (reuses existing function)
    let all_collections = list_collections(pool).await?;

    // 2. Find referencing pairs: (collection_name, field_definition) where
    //    the field is a Relationship type pointing to collection_name
    let mut referencing_pairs: Vec<(&CollectionDefinition, &FieldDefinition)> = Vec::new();
    for collection in &all_collections {
        for field in &collection.fields {
            if field.field_type == FieldType::Relationship {
                if let Some(ref rc) = field.related_collection {
                    // Skip 1:M fields — they are virtual (no FK column exists).
                    // The actual FK is on the M:1 side of the relationship.
                    if rc == collection_name
                        && field.relationship_type.as_deref() != Some("one_to_many")
                    {
                        referencing_pairs.push((collection, field));
                    }
                }
            }
        }
    }

    // 3. Query each referencing collection for items where FK = item_id
    let mut results: Vec<ReferencingGroup> = Vec::new();
    for (collection, field) in referencing_pairs {
        // Check if user has permission to this collection
        let perms = match collection_filters.get(&collection.name) {
            Some(entry) => entry,
            None => continue, // No permission → skip this referencing collection
        };

        let quoted_table = quote_id(&collection.name);
        let quoted_fk_col = quote_id(&field.name);

        // Compile permission filter at offset 1 (leaving $1 for item_id)
        let (where_part, extra_binds): (String, Vec<serde_json::Value>) = if perms.is_empty() {
            (format!("{} = $1::uuid", quoted_fk_col), vec![])
        } else {
            let (clause, binds) = crate::services::permissions::build_filter_clause_with_offset(
                perms,
                1,
                None,
                Some(collection),
            );
            if clause.is_empty() {
                (format!("{} = $1::uuid", quoted_fk_col), vec![])
            } else {
                (
                    format!("({}) = $1::uuid AND ({})", quoted_fk_col, clause),
                    binds,
                )
            }
        };

        let sql = format!(
            "SELECT COALESCE(json_agg(\"_q\"), '[]'::json) FROM (SELECT * FROM {} WHERE {}) AS \"_q\"",
            quoted_table, where_part,
        );

        // Bind order: item_id first ($1), then permission binds ($2, $3...)
        let mut query = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
        query = query.bind(item_id);
        for val in &extra_binds {
            query = crate::bind_json_value!(query, val);
        }
        let (raw_result,): (serde_json::Value,) =
            query
                .fetch_one(pool)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!(
                        "Reverse lookup query failed for {}::{}: {}",
                        collection.name, field.name, e
                    ),
                })?;

        let items = match raw_result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        results.push(ReferencingGroup {
            collection_name: collection.name.clone(),
            field_name: field.name.clone(),
            relationship_type: field
                .relationship_type
                .clone()
                .unwrap_or_else(|| "many_to_one".to_string()),
            items,
        });
    }

    Ok(results)
}

/// Like get_collection but uses Redis cache. Falls back to DB on cache miss or Redis error.
pub async fn get_cached_collection(
    pool: &Pool,
    redis: &Option<crate::services::redis_session::RedisPool>,
    name: &str,
) -> Result<CollectionDefinition, AppError> {
    let key = format!("schema:collection:{}", name);
    if let Some(cached) = crate::services::cache::try_get(redis, &key).await {
        if let Ok(collection) = serde_json::from_str(&cached) {
            return Ok(collection);
        }
    }
    let collection = get_collection(pool, name).await?;
    if let Ok(json) = serde_json::to_string(&collection) {
        crate::services::cache::try_set(redis, &key, &json, 300).await;
    }
    Ok(collection)
}

/// Like list_collections but uses Redis cache.
pub async fn get_cached_collections(
    pool: &Pool,
    redis: &Option<crate::services::redis_session::RedisPool>,
) -> Result<Vec<CollectionDefinition>, AppError> {
    let key = "schema:all";
    if let Some(cached) = crate::services::cache::try_get(redis, key).await {
        if let Ok(collections) = serde_json::from_str(&cached) {
            return Ok(collections);
        }
    }
    let collections = list_collections(pool).await?;
    if let Ok(json) = serde_json::to_string(&collections) {
        crate::services::cache::try_set(redis, key, &json, 300).await;
    }
    Ok(collections)
}
