use std::collections::HashMap;

use alcedo_common::context::AppContext;
use alcedo_common::error::AppError;
use alcedo_common::state::{CoreDatabaseSchema, CoreForeignKey, CoreState};

use crate::db::collections::{self, CollectionDefinition, FieldType};
use crate::db::ALCEDO_SCHEMA;

#[derive(Debug, Clone)]
pub struct TableRef {
    pub schema: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ColumnShape {
    pub name: String,
    pub field_type: FieldType,
    /// Canonicalized PostgreSQL data type (e.g. `text[]`, `jsonb`, `uuid`),
    /// used for physical casts of JSON and array columns.
    pub data_type: String,
    pub is_nullable: bool,
    /// Derived by the inspector from index definitions, so it can be true for
    /// members of a composite unique index or a primary key. Informational
    /// only, not a hard per-column constraint.
    pub is_unique: bool,
    pub is_pk: bool,
    pub has_default: bool,
    /// True when the column is backed by a sequence (`nextval(...)` default).
    pub has_auto_increment: bool,
    pub foreign_key: Option<CoreForeignKey>,
}

#[derive(Debug, Clone)]
pub struct TableShape {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnShape>,
    pub pk: Vec<String>,
    pub collection: Option<CollectionDefinition>,
    /// Default-projection guard: columns that may be projected when `fields` is
    /// empty. `None` = `*`. The shape's primary-key column(s) are always unioned
    /// into the projection so row identity survives. Explicit `fields` are still
    /// validated for column existence only; the caller/boundary is responsible
    /// for what it requests and serializes.
    pub readable: Option<Vec<String>>,
    /// Columns that may be written. `None` = any existing column.
    pub writable: Option<Vec<String>>,
}

impl TableShape {
    pub async fn resolve(
        core: &CoreState,
        context: &AppContext,
        reference: TableRef,
    ) -> Result<TableShape, AppError> {
        let app_schema = context.schema_name();
        let schema = reference
            .schema
            .clone()
            .unwrap_or_else(|| app_schema.clone());

        // PK order follows column order; composite PKs are deferred.
        let (mut columns, pk) = {
            let guard = core.schema.read().await;
            let mut columns = Vec::new();
            let mut pk = Vec::new();

            for column in guard
                .columns
                .iter()
                .filter(|c| c.schema == schema && c.table == reference.name)
            {
                columns.push(ColumnShape {
                    name: column.name.clone(),
                    field_type: physical_field_type(&column.data_type),
                    data_type: column.data_type.clone(),
                    is_nullable: column.is_nullable,
                    is_unique: column.is_unique,
                    is_pk: column.is_primary_key,
                    has_default: column.default_value.is_some(),
                    has_auto_increment: column.has_auto_increment,
                    foreign_key: column.foreign_key.clone(),
                });

                if column.is_primary_key {
                    pk.push(column.name.clone());
                }
            }

            (columns, pk)
        };

        if columns.is_empty() {
            return Err(AppError::NotFound(format!(
                "Table '{}.{}' not found",
                schema, reference.name
            )));
        }

        // Global `alcedo` tables are physical-only: the app schema may define a
        // same-named system collection (e.g. `alcedo_users`), and attaching its
        // metadata would wrongly route global tables through the collection path.
        let collection = if schema == app_schema && schema != ALCEDO_SCHEMA {
            match core.pool() {
                Ok(pool) => match collections::get_collection(pool, &reference.name).await {
                    Ok(collection) => Some(collection),
                    Err(AppError::NotFound(_)) => None,
                    Err(e) => return Err(e),
                },
                Err(e) => return Err(e),
            }
        } else {
            None
        };

        if let Some(collection) = &collection {
            for column in &mut columns {
                if let Some(field) = collection.fields.iter().find(|f| f.name == column.name) {
                    column.field_type = field.field_type.clone();
                }
            }
        }

        let (readable, writable) = allowlists_for(&schema, &reference.name);

        Ok(TableShape {
            schema,
            name: reference.name,
            columns,
            pk,
            collection,
            readable,
            writable,
        })
    }

    pub fn col_type_map_ref(&self) -> HashMap<&str, &FieldType> {
        self.columns
            .iter()
            .map(|c| (c.name.as_str(), &c.field_type))
            .collect()
    }

    /// Resolve the table's single primary-key column.
    ///
    /// The shape-based read/write paths support only single-column primary
    /// keys; composite keys return a `BadRequest`.
    pub fn single_pk(&self) -> Result<&ColumnShape, AppError> {
        if self.pk.len() != 1 {
            return Err(AppError::BadRequest(format!(
                "Table '{}.{}' must have exactly one primary key column (found {})",
                self.schema,
                self.name,
                self.pk.len()
            )));
        }

        let pk_name = &self.pk[0];
        self.columns
            .iter()
            .find(|c| &c.name == pk_name)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Primary key column '{}' not found in table '{}.{}'",
                    pk_name, self.schema, self.name
                ))
            })
    }
}

/// Readable/writable column allowlists for sensitive global tables.
///
/// `readable` gates empty-field projections so they can never `SELECT *`
/// over columns like `password_hash`; `writable` rejects writes to columns
/// that must stay server-managed. `None` in either position preserves the
/// historical permissive behavior.
fn allowlists_for(schema: &str, table: &str) -> (Option<Vec<String>>, Option<Vec<String>>) {
    match (schema, table) {
        (ALCEDO_SCHEMA, "alcedo_users") => (
            Some(vec![
                "id".into(),
                "email".into(),
                "display_name".into(),
                "is_admin".into(),
                "last_login_at".into(),
                "created_at".into(),
                "updated_at".into(),
            ]),
            Some(vec![
                "email".into(),
                "display_name".into(),
                "is_admin".into(),
                "last_login_at".into(),
            ]),
        ),
        (ALCEDO_SCHEMA, "alcedo_developer_api_keys") => (
            Some(vec![
                "id".into(),
                "name".into(),
                "version_id".into(),
                "key_prefix".into(),
                "is_active".into(),
                "created_at".into(),
                "last_used_at".into(),
            ]),
            Some(vec![
                "name".into(),
                "version_id".into(),
                "is_active".into(),
                "last_used_at".into(),
            ]),
        ),
        (ALCEDO_SCHEMA, "alcedo_registries") => (
            Some(vec![
                "id".into(),
                "name".into(),
                "url".into(),
                "pull_url".into(),
                "auth_type".into(),
                "created_at".into(),
                "updated_at".into(),
            ]),
            Some(vec![
                "name".into(),
                "url".into(),
                "pull_url".into(),
                "auth_type".into(),
                "username".into(),
                "password".into(),
            ]),
        ),
        (ALCEDO_SCHEMA, "alcedo_plugins") => (
            Some(vec![
                "id".into(),
                "slug".into(),
                "app_version_id".into(),
                "version_id".into(),
                "image".into(),
                "plugin_type".into(),
                "system_plugin".into(),
                "resources".into(),
                "display_name".into(),
                "description".into(),
                "pages".into(),
                "endpoints".into(),
                "documentation".into(),
                "settings_schema".into(),
                "settings".into(),
                "tags".into(),
                "enabled".into(),
                "registry_id".into(),
                "requested_scopes".into(),
                "granted_scopes".into(),
                "created_at".into(),
                "updated_at".into(),
            ]),
            None,
        ),
        (_, "alcedocore_roles") => (
            // App-schema tables have a dynamic schema, so match on table name
            // only. This arm MUST precede the `_` fallback — an arm bound to
            // `ALCEDO_SCHEMA` for an app table would silently never match.
            Some(vec![
                "id".into(),
                "name".into(),
                "description".into(),
                "is_system".into(),
                "created_at".into(),
                "updated_at".into(),
            ]),
            Some(vec!["name".into(), "description".into()]),
        ),
        _ => (None, None),
    }
}

/// Quote a schema-qualified table identifier.
pub(crate) fn qualified_table(schema: &str, name: &str) -> String {
    format!(
        "{}.{}",
        crate::db::filter_compiler::quote(schema),
        crate::db::filter_compiler::quote(name)
    )
}

/// Quote a table reference, schema-qualifying it when a schema is known and
/// falling back to the bare identifier otherwise.
pub(crate) fn qualified_table_ref(schema: Option<&str>, name: &str) -> String {
    match schema {
        Some(schema) => qualified_table(schema, name),
        None => crate::db::filter_compiler::quote(name),
    }
}

/// Physical cast for a column, derived from its `FieldType` and raw PG type.
///
/// Array columns carry `FieldType::File` regardless of element type, so the
/// concrete PG element type is taken from `data_type` (e.g. `text[]`).
pub fn column_cast(column: &ColumnShape) -> &'static str {
    match column.field_type {
        FieldType::Uuid | FieldType::Relationship => "::uuid",
        FieldType::Int => "::bigint",
        FieldType::Float => "::float8",
        FieldType::Datetime => "::timestamptz",
        FieldType::Bool => "::bool",
        FieldType::File => array_type_cast(&column.data_type),
        FieldType::String | FieldType::Text => match column.data_type.as_str() {
            "json" | "jsonb" => "::jsonb",
            "text[]" | "character varying[]" | "varchar[]" | "character[]" => "::text[]",
            "integer[]" | "int[]" | "bigint[]" | "smallint[]" | "int2[]" | "int4[]" | "int8[]" => {
                "::bigint[]"
            }
            "boolean[]" | "bool[]" => "::bool[]",
            "uuid[]" => "::uuid[]",
            _ => "",
        },
    }
}

/// Physical cast for an array column, derived from the raw PG element type.
///
/// UUID arrays cast to `::uuid[]`; integer-family arrays widen to `::bigint[]`
/// so smallint/int values bind cleanly; every other array binds its Postgres
/// array literal without a cast, letting the column type drive inference.
fn array_type_cast(data_type: &str) -> &'static str {
    let element = data_type.strip_suffix("[]").unwrap_or(data_type);
    match element {
        "uuid" => "::uuid[]",
        "text" | "varchar" | "character varying" | "character" => "::text[]",
        "int" | "int2" | "int4" | "int8" | "integer" | "smallint" | "bigint" => "::bigint[]",
        "bool" | "boolean" => "::bool[]",
        "float" | "float4" | "float8" | "real" | "double precision" => "::float8[]",
        _ => "",
    }
}

/// Coerce a value for a physical (non-collection) table column. Array columns
/// are converted to PostgreSQL array literals; json/jsonb scalars are
/// JSON-encoded to strings so they bind against `::jsonb`; everything else
/// delegates to `collection_items::coerce_value`.
pub fn coerce_physical_value(value: serde_json::Value, column: &ColumnShape) -> serde_json::Value {
    if column.data_type.ends_with("[]") && column.data_type != "uuid[]" {
        match value {
            serde_json::Value::Array(arr) => {
                let elems: Vec<String> = arr
                    .iter()
                    .map(|v| match v {
                        serde_json::Value::Null => "NULL".to_string(),
                        serde_json::Value::String(s) => {
                            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
                        }
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        _ => "NULL".to_string(),
                    })
                    .collect();
                serde_json::Value::String(format!("{{{}}}", elems.join(",")))
            }
            serde_json::Value::String(s) if s.is_empty() => serde_json::Value::String("{}".to_string()),
            _ => crate::db::collection_items::coerce_value(value, &column.field_type),
        }
    } else if matches!(column.data_type.as_str(), "json" | "jsonb") {
        match value {
            serde_json::Value::Null => serde_json::Value::Null,
            // Scalars must be JSON-encoded (e.g. `"str"`, `42`) to bind against `::jsonb`.
            _ => serde_json::Value::String(
                serde_json::to_string(&value)
                    .expect("serde_json::to_string on Value is infallible"),
            ),
        }
    } else {
        crate::db::collection_items::coerce_value(value, &column.field_type)
    }
}

/// Validate write keys against a [`TableShape`].
///
/// Collection-backed shapes delegate to
/// [`crate::db::collection_items::validate_fields_for_write`], preserving the
/// reserved `id`/`created_at`/`updated_at` rules. Physical (system/meta) shapes
/// have no collection metadata and only enforce that each key is an existing
/// column — writes to those tables are gated by the caller at the boundary.
pub fn validate_fields_for_write_shape(
    keys: &[String],
    shape: &TableShape,
) -> Result<(), AppError> {
    if let Some(collection) = &shape.collection {
        return crate::db::collection_items::validate_fields_for_write(keys, collection);
    }

    for key in keys {
        if !shape.columns.iter().any(|c| c.name == *key) {
            return Err(AppError::BadRequest(format!("Unknown field: '{}'", key)));
        }
        if let Some(writable) = &shape.writable {
            if !writable.iter().any(|c| c == key) {
                return Err(AppError::BadRequest(format!(
                    "Field '{}' is not writable",
                    key
                )));
            }
        }
    }

    Ok(())
}

/// A relation discovered purely from physical foreign-key constraints.
#[derive(Debug, Clone)]
pub struct PhysicalRelation {
    pub target_schema: String,
    pub target_table: String,
    pub fk_column: String,
    pub pk_column: String,
}

/// Owned snapshot of physical FK metadata, safe to pass to synchronous
/// relation detectors without holding the async schema lock.
#[derive(Debug, Clone, Default)]
pub struct PhysicalCatalog {
    column_fk: HashMap<(String, String, String), CoreForeignKey>,
    table_fk_columns: HashMap<(String, String), Vec<(String, CoreForeignKey)>>,
    table_pk: HashMap<(String, String), String>,
    table_schemas_by_name: HashMap<String, Vec<String>>,
}

impl PhysicalCatalog {
    pub fn from_schema(schema: &CoreDatabaseSchema) -> Self {
        let mut catalog = Self::default();

        for column in &schema.columns {
            let table_key = (column.schema.clone(), column.table.clone());

            if column.is_primary_key {
                catalog
                    .table_pk
                    .entry(table_key.clone())
                    .or_insert_with(|| column.name.clone());
            }

            let schemas = catalog
                .table_schemas_by_name
                .entry(column.table.clone())
                .or_default();
            if !schemas.contains(&column.schema) {
                schemas.push(column.schema.clone());
            }

            if let Some(fk) = &column.foreign_key {
                catalog.column_fk.insert(
                    (
                        column.schema.clone(),
                        column.table.clone(),
                        column.name.clone(),
                    ),
                    fk.clone(),
                );
                catalog
                    .table_fk_columns
                    .entry(table_key)
                    .or_default()
                    .push((column.name.clone(), fk.clone()));
            }
        }

        catalog
    }

    /// Resolve a table's schema from the snapshot, preferring `preferred_schema`
    /// and falling back to the `alcedo` schema. Returns `None` when neither is
    /// present so callers can keep an unqualified table reference.
    pub fn table_schema(&self, table: &str, preferred_schema: &str) -> Option<String> {
        let schemas = self.table_schemas_by_name.get(table)?;
        if schemas.iter().any(|s| s == preferred_schema) {
            return Some(preferred_schema.to_string());
        }
        if schemas.iter().any(|s| s == ALCEDO_SCHEMA) {
            return Some(ALCEDO_SCHEMA.to_string());
        }
        None
    }

    /// M:1 forward — a base column named `segment` carries the FK.
    pub fn many_to_one(
        &self,
        base_schema: &str,
        base_table: &str,
        segment: &str,
    ) -> Option<PhysicalRelation> {
        let fk = self.column_fk.get(&(
            base_schema.to_string(),
            base_table.to_string(),
            segment.to_string(),
        ))?;

        Some(PhysicalRelation {
            target_schema: fk.schema.clone(),
            target_table: fk.table.clone(),
            fk_column: segment.to_string(),
            pk_column: fk.column.clone(),
        })
    }

    /// 1:M reverse — a table named `segment` holds a column whose FK points
    /// back at `(base_schema, base_table, base PK)`.
    pub fn one_to_many(
        &self,
        base_schema: &str,
        base_table: &str,
        segment: &str,
    ) -> Option<PhysicalRelation> {
        let base_pk = self
            .table_pk
            .get(&(base_schema.to_string(), base_table.to_string()));

        for candidate_schema in self.candidate_schemas(base_schema, segment) {
            let columns = match self
                .table_fk_columns
                .get(&(candidate_schema.clone(), segment.to_string()))
            {
                Some(columns) => columns,
                None => continue,
            };

            for (column, fk) in columns {
                if fk.schema != base_schema || fk.table != base_table {
                    continue;
                }
                if let Some(pk) = base_pk {
                    if &fk.column != pk {
                        continue;
                    }
                }

                return Some(PhysicalRelation {
                    target_schema: candidate_schema,
                    target_table: segment.to_string(),
                    fk_column: column.clone(),
                    pk_column: base_pk.cloned().unwrap_or_else(|| fk.column.clone()),
                });
            }
        }

        None
    }

    fn candidate_schemas(&self, base_schema: &str, segment: &str) -> Vec<String> {
        let schemas = match self.table_schemas_by_name.get(segment) {
            Some(schemas) => schemas,
            None => return Vec::new(),
        };

        let mut ordered = Vec::new();
        if schemas.iter().any(|s| s == base_schema) {
            ordered.push(base_schema.to_string());
        }
        if base_schema != ALCEDO_SCHEMA && schemas.iter().any(|s| s == ALCEDO_SCHEMA) {
            ordered.push(ALCEDO_SCHEMA.to_string());
        }
        ordered
    }
}

/// Maps `information_schema.columns.data_type` to a [`FieldType`].
///
/// SQL-standard spellings are expected (`integer`, `character varying`,
/// `double precision`, `timestamp with time zone`, `ARRAY`); the additional
/// aliases (`int4`, `varchar`, `bool`, `serial`, ...) are defensive.
fn physical_field_type(data_type: &str) -> FieldType {
    let data_type = data_type.to_ascii_lowercase();

    if data_type.ends_with("[]") || data_type == "array" {
        return FieldType::File;
    }

    match data_type.as_str() {
        "uuid" => FieldType::Uuid,
        "date" => FieldType::Datetime,
        t if t.starts_with("timestamp") => FieldType::Datetime,
        "int" | "int2" | "int4" | "int8" | "integer" | "smallint" | "bigint" | "serial"
        | "serial2" | "serial4" | "serial8" | "bigserial" | "smallserial" => FieldType::Int,
        "numeric" | "decimal" | "real" | "double precision" | "float" | "float4" | "float8" => {
            FieldType::Float
        }
        "boolean" | "bool" => FieldType::Bool,
        "text" => FieldType::Text,
        t if t.starts_with("character") => FieldType::String,
        "varchar" => FieldType::String,
        _ => FieldType::String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_field_type_covers_sql_standard_and_aliases() {
        let cases: &[(&str, FieldType)] = &[
            ("uuid", FieldType::Uuid),
            ("timestamp without time zone", FieldType::Datetime),
            ("timestamp with time zone", FieldType::Datetime),
            ("date", FieldType::Datetime),
            ("integer", FieldType::Int),
            ("smallint", FieldType::Int),
            ("bigint", FieldType::Int),
            ("serial", FieldType::Int),
            ("numeric", FieldType::Float),
            ("double precision", FieldType::Float),
            ("real", FieldType::Float),
            ("boolean", FieldType::Bool),
            ("text", FieldType::Text),
            ("character varying", FieldType::String),
            ("character", FieldType::String),
            ("ARRAY", FieldType::File),
            ("uuid[]", FieldType::File),
            ("jsonb", FieldType::String),
        ];

        for (data_type, expected) in cases {
            assert_eq!(
                &physical_field_type(data_type),
                expected,
                "unexpected mapping for {}",
                data_type
            );
        }
    }
}
