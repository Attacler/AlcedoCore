use crate::db::collections::{FieldDefinition, FieldType};
use crate::db::Pool;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FieldRow {
    pub id: Uuid,
    pub collection_name: String,
    pub name: String,
    pub display_name: Option<String>,
    pub field_type: String,
    pub required: bool,
    pub unique_constraint: bool,
    pub default_value: Option<serde_json::Value>,
    pub display_type: Option<String>,
    pub input_component: Option<String>,
    pub display_component: Option<String>,
    pub ordinal_position: i32,
    pub related_collection: Option<String>,
    pub relationship_type: Option<String>,
    pub display_field: Option<String>,
    pub inline_parent_fields: Option<serde_json::Value>,
    pub options: Option<serde_json::Value>,
    pub is_system: bool,
    pub hidden: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn field_type_to_str(ft: &FieldType) -> String {
    match ft {
        FieldType::String => "string".to_string(),
        FieldType::Text => "text".to_string(),
        FieldType::Int => "int".to_string(),
        FieldType::Float => "float".to_string(),
        FieldType::Datetime => "datetime".to_string(),
        FieldType::Uuid => "uuid".to_string(),
        FieldType::Relationship => "relationship".to_string(),
        FieldType::Bool => "boolean".to_string(),
        FieldType::File => "file".to_string(),
    }
}

fn str_to_field_type(s: &str) -> FieldType {
    match s {
        "string" => FieldType::String,
        "text" => FieldType::Text,
        "int" => FieldType::Int,
        "float" => FieldType::Float,
        "datetime" => FieldType::Datetime,
        "uuid" => FieldType::Uuid,
        "relationship" => FieldType::Relationship,
        "boolean" => FieldType::Bool,
        "file" => FieldType::File,
        _ => FieldType::String,
    }
}

impl FieldRow {
    pub fn to_definition(&self) -> FieldDefinition {
        let options: Option<serde_json::Value> = self.options.clone().filter(|v| {
            if let Some(arr) = v.as_array() {
                !arr.is_empty()
            } else {
                true
            }
        });

        let inline_parent: Option<Vec<String>> = self
            .inline_parent_fields
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .filter(|v: &Vec<String>| !v.is_empty());

        FieldDefinition {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            field_type: str_to_field_type(&self.field_type),
            required: self.required,
            unique: self.unique_constraint,
            default: self.default_value.clone(),
            display_type: self.display_type.clone(),
            input_component: self.input_component.clone(),
            display_component: self.display_component.clone(),
            related_collection: self.related_collection.clone(),
            relationship_type: self.relationship_type.clone(),
            display_field: self.display_field.clone(),
            inline_parent_fields: inline_parent,
            is_system: self.is_system,
            hidden: self.hidden,
            options,
        }
    }

    pub fn from_definition(def: &FieldDefinition, collection_name: &str, ordinal: i32) -> Self {
        let options_json = def.options.clone();
        let inline_parent_json = def
            .inline_parent_fields
            .as_ref()
            .and_then(|v| serde_json::to_value(v).ok());

        FieldRow {
            id: Uuid::nil(),
            collection_name: collection_name.to_string(),
            name: def.name.clone(),
            display_name: def.display_name.clone(),
            field_type: field_type_to_str(&def.field_type),
            required: def.required,
            unique_constraint: def.unique,
            default_value: def.default.clone(),
            display_type: def.display_type.clone(),
            input_component: def.input_component.clone(),
            display_component: def.display_component.clone(),
            ordinal_position: ordinal,
            related_collection: def.related_collection.clone(),
            relationship_type: def.relationship_type.clone(),
            display_field: def.display_field.clone(),
            inline_parent_fields: inline_parent_json,
            options: options_json,
            is_system: def.is_system,
            hidden: def.hidden,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}

pub async fn list_fields_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    collection_name: &str,
) -> Result<Vec<FieldRow>, AppError> {
    let rows = sqlx::query_as::<_, FieldRow>(
        "SELECT id, collection_name, name, display_name, field_type, required, \
         unique_constraint, default_value, display_type, input_component, display_component, ordinal_position, \
         related_collection, relationship_type, display_field, inline_parent_fields, \
         options, is_system, hidden, created_at, updated_at \
         FROM collection_fields \
         WHERE collection_name = $1 \
         ORDER BY ordinal_position ASC"
    )
    .bind(collection_name)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to list fields for '{}': {}", collection_name, e),
    })?;
    Ok(rows)
}

pub async fn list_fields(pool: &Pool, collection_name: &str) -> Result<Vec<FieldRow>, AppError> {
    let rows = sqlx::query_as::<_, FieldRow>(
        "SELECT id, collection_name, name, display_name, field_type, required, \
         unique_constraint, default_value, display_type, input_component, display_component, ordinal_position, \
         related_collection, relationship_type, display_field, inline_parent_fields, \
         options, is_system, hidden, created_at, updated_at \
         FROM collection_fields \
         WHERE collection_name = $1 \
         ORDER BY ordinal_position ASC"
    )
    .bind(collection_name)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to list fields for '{}': {}", collection_name, e),
    })?;
    Ok(rows)
}

pub async fn replace_fields_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    collection_name: &str,
    fields: &[FieldDefinition],
) -> Result<Vec<FieldRow>, AppError> {
    sqlx::query("DELETE FROM collection_fields WHERE collection_name = $1")
        .bind(collection_name)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to clear fields for '{}': {}", collection_name, e),
        })?;

    let mut results = Vec::new();
    for (i, def) in fields.iter().enumerate() {
        let row = FieldRow::from_definition(def, collection_name, i as i32 + 1);
        let inline_parent_str = match &row.inline_parent_fields {
            Some(v) => serde_json::to_string(v).map_err(|e| {
                AppError::Internal(format!("Failed to serialize inline_parent_fields: {}", e))
            })?,
            None => "[]".to_string(),
        };
        let options_str = match &row.options {
            Some(v) => serde_json::to_string(v)
                .map_err(|e| AppError::Internal(format!("Failed to serialize options: {}", e)))?,
            None => "[]".to_string(),
        };
        let default_str = match &row.default_value {
            Some(v) => serde_json::to_string(v).map_err(|e| {
                AppError::Internal(format!("Failed to serialize default_value: {}", e))
            })?,
            None => "null".to_string(),
        };

        let inserted = sqlx::query_as::<_, FieldRow>(
            "INSERT INTO collection_fields \
             (collection_name, name, display_name, field_type, required, unique_constraint, \
              default_value, display_type, input_component, display_component, ordinal_position, related_collection, \
              relationship_type, display_field, inline_parent_fields, options, \
              is_system, hidden) \
             VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8, $9, $10, $11, $12, $13, $14, $15::jsonb, $16::jsonb, \
                     $17, $18) \
             RETURNING id, collection_name, name, display_name, field_type, required, \
                       unique_constraint, default_value, display_type, input_component, display_component, ordinal_position, \
                       related_collection, relationship_type, display_field, inline_parent_fields, \
                       options, is_system, hidden, created_at, updated_at"
        )
        .bind(&row.collection_name)
        .bind(&row.name)
        .bind(&row.display_name)
        .bind(&row.field_type)
        .bind(row.required)
        .bind(row.unique_constraint)
        .bind(&default_str)
        .bind(&row.display_type)
        .bind(&row.input_component)
        .bind(&row.display_component)
        .bind(row.ordinal_position)
        .bind(&row.related_collection)
        .bind(&row.relationship_type)
        .bind(&row.display_field)
        .bind(&inline_parent_str)
        .bind(&options_str)
        .bind(row.is_system)
        .bind(row.hidden)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to insert field '{}': {}", def.name, e),
        })?;
        results.push(inserted);
    }

    Ok(results)
}

pub async fn delete_field(
    pool: &Pool,
    collection_name: &str,
    field_name: &str,
) -> Result<bool, AppError> {
    let result =
        sqlx::query("DELETE FROM collection_fields WHERE collection_name = $1 AND name = $2")
            .bind(collection_name)
            .bind(field_name)
            .execute(pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to delete field '{}': {}", field_name, e),
            })?;

    sqlx::query("UPDATE collection_definitions SET updated_at = NOW() WHERE name = $1")
        .bind(collection_name)
        .execute(pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to update collection timestamp: {}", e),
        })?;

    Ok(result.rows_affected() > 0)
}
