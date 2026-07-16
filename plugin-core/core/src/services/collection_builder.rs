use sea_query::{Alias, ColumnDef, Expr, ForeignKey, ForeignKeyAction, PostgresQueryBuilder, SimpleExpr, Table};
use std::collections::HashSet;

use crate::db::collections::{FieldDefinition, FieldType};
use crate::error::AppError;

/// Pure DDL SQL generation service.
/// Generates PostgreSQL DDL statements via sea-query — no raw string interpolation.
/// All methods return SQL strings; callers execute them via sqlx within transactions.
pub struct CollectionBuilder;

impl CollectionBuilder {
    /// Check if a field is a virtual 1:M relationship field.
    /// Virtual fields do NOT generate columns on the source table — the FK
    /// column is created on the target (child) table instead.
    pub fn is_virtual_field(field: &FieldDefinition) -> bool {
        field.field_type == FieldType::Relationship
            && field.relationship_type.as_deref() == Some("one_to_many")
    }

    /// Generate CREATE TABLE statement with:
    /// - Implicit columns: id UUID PK, created_at, updated_at (per locked decisions)
    /// - User-defined field columns with proper types and constraints
    /// Returns the SQL string, ready for sqlx::query().execute()
    pub fn build_create_table_stmt(name: &str, fields: &[FieldDefinition]) -> Result<String, AppError> {
        let mut stmt = Table::create()
            .table(Alias::new(name))
            .to_owned();

        // Implicit columns per locked decisions
        stmt.col(
            ColumnDef::new(Alias::new("id"))
                .uuid()
                .not_null()
                .primary_key()
                .default(Expr::cust("gen_random_uuid()"))
        );
        stmt.col(
            ColumnDef::new(Alias::new("created_at"))
                .timestamp_with_time_zone()
                .not_null()
                .default(Expr::cust("NOW()"))
        );
        stmt.col(
            ColumnDef::new(Alias::new("updated_at"))
                .timestamp_with_time_zone()
                .not_null()
                .default(Expr::cust("NOW()"))
        );

        for field in fields.iter().filter(|f| !Self::is_virtual_field(f)) {
            let col_def = Self::field_to_column_def(field)?;
            stmt.col(col_def);
        }

        let sql = stmt.to_string(PostgresQueryBuilder);
        Ok(sql)
    }

    /// Generate DROP TABLE IF EXISTS statement.
    pub fn build_drop_table_stmt(name: &str) -> String {
        let sql = Table::drop()
            .table(Alias::new(name))
            .if_exists()
            .to_string(PostgresQueryBuilder);
        sql
    }

    /// Generate ALTER TABLE ADD COLUMN for each field added.
    /// Returns a Vec of SQL statements (one per new column).
    pub fn build_add_columns_stmt(name: &str, fields: &[&FieldDefinition]) -> Result<Vec<String>, AppError> {
        let mut statements = Vec::new();
        for field in fields.iter().copied().filter(|f| !Self::is_virtual_field(f)) {
            let col_def = Self::field_to_column_def(field)?;
            let sql = Table::alter()
                .table(Alias::new(name))
                .add_column(col_def)
                .to_string(PostgresQueryBuilder);
            statements.push(sql);
        }
        Ok(statements)
    }

    /// Generate ALTER TABLE DROP COLUMN for each field removed.
    /// Returns a Vec of SQL statements (one per removed column).
    pub fn build_drop_columns_stmt(name: &str, field_names: &[&str]) -> Vec<String> {
        field_names.iter().map(|field_name| {
            Table::alter()
                .table(Alias::new(name))
                .drop_column(Alias::new(*field_name))
                .to_string(PostgresQueryBuilder)
        }).collect()
    }

    /// Check if a field's type-relevant properties changed between old and new.
    /// Detects changes to field_type, related_collection, or relationship_type
    /// that require DDL changes (ALTER TABLE ADD/DROP COLUMN or FK changes).
    fn field_props_changed(old: &FieldDefinition, new: &FieldDefinition) -> bool {
        old.field_type != new.field_type
            || old.related_collection != new.related_collection
            || old.relationship_type != new.relationship_type
    }

    /// Compute field changes between old and new field definitions.
    /// Returns (renamed_pairs, added_fields, removed_field_names).
    ///
    /// Detection rules:
    /// 1. Same name in both → no change
    /// 2. Unmatched old + unmatched new with same type (and same
    ///    related_collection/relationship_type for relationships) → rename
    /// 3. Truly unmatched old → removed (DROP COLUMN)
    /// 4. Truly unmatched new → added (ADD COLUMN)
    /// 5. Same name but type properties changed → treated as remove + re-add
    pub fn compute_field_changes<'a>(
        old_fields: &'a [FieldDefinition],
        new_fields: &'a [FieldDefinition],
    ) -> (Vec<(&'a str, &'a str)>, Vec<&'a FieldDefinition>, Vec<&'a str>) {
        // Filter out system fields — they can't be added/removed/altered
        let old_custom: Vec<&'a FieldDefinition> = old_fields.iter().filter(|f| !f.is_system).collect();
        let new_custom: Vec<&'a FieldDefinition> = new_fields.iter().filter(|f| !f.is_system).collect();

        let old_names: HashSet<&str> = old_custom.iter().map(|f| f.name.as_str()).collect();
        let new_names: HashSet<&str> = new_custom.iter().map(|f| f.name.as_str()).collect();

        // Fields whose type properties changed (same name, different type/etc)
        let changed_new: Vec<&FieldDefinition> = new_custom.iter().copied()
            .filter(|f| {
                old_names.contains(f.name.as_str())
                    && old_custom.iter().any(|o| o.name == f.name && Self::field_props_changed(o, f))
            })
            .collect();

        let changed_old_names: HashSet<&str> = changed_new.iter()
            .map(|f| f.name.as_str())
            .collect();

        // Unmatched: old names not in new, new names not in old (excluding type-changed)
        let unmatched_old: Vec<&FieldDefinition> = old_custom.iter().copied()
            .filter(|f| !new_names.contains(f.name.as_str()) && !changed_old_names.contains(f.name.as_str()))
            .collect();
        let unmatched_new: Vec<&FieldDefinition> = new_custom.iter().copied()
            .filter(|f| !old_names.contains(f.name.as_str()) && !changed_old_names.contains(f.name.as_str()))
            .collect();

        // Detect renames: match unmatched old ↔ unmatched new by compatible properties
        let mut renamed: Vec<(&'a str, &'a str)> = Vec::new();
        let mut used_old: Vec<bool> = vec![false; unmatched_old.len()];
        let mut used_new: Vec<bool> = vec![false; unmatched_new.len()];

        for (oi, old) in unmatched_old.iter().enumerate() {
            for (ni, new) in unmatched_new.iter().enumerate() {
                if used_old[oi] || used_new[ni] {
                    continue;
                }
                let match_ok = if old.field_type == FieldType::Relationship {
                    old.field_type == new.field_type
                        && old.related_collection == new.related_collection
                        && old.relationship_type == new.relationship_type
                } else {
                    old.field_type == new.field_type
                };
                if match_ok {
                    renamed.push((old.name.as_str(), new.name.as_str()));
                    used_old[oi] = true;
                    used_new[ni] = true;
                }
            }
        }

        // Remaining unmatched = truly added / removed
        let added: Vec<&FieldDefinition> = unmatched_new.iter()
            .enumerate()
            .filter(|(i, _)| !used_new[*i])
            .map(|(_, f)| *f)
            .chain(changed_new.iter().copied())
            .collect();

        let removed: Vec<&str> = unmatched_old.iter()
            .enumerate()
            .filter(|(i, _)| !used_old[*i])
            .map(|(_, f)| f.name.as_str())
            .chain(changed_old_names.iter().copied())
            .collect();

        (renamed, added, removed)
    }

    /// Generate ALTER TABLE RENAME COLUMN for each renamed field.
    /// Preserves all existing data in the column.
    pub fn build_rename_columns_stmt(name: &str, renames: &[(&str, &str)]) -> Vec<String> {
        renames.iter().map(|(old_name, new_name)| {
            format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {}",
                crate::db::quote_identifier(name),
                crate::db::quote_identifier(old_name),
                crate::db::quote_identifier(new_name),
            )
        }).collect()
    }

    /// Generate ALTER TABLE statements to rename 1:M FK columns on child tables
    /// when the source field is renamed.
    pub fn build_rename_o2m_fk_sqls(
        source_table: &str,
        renames: &[(&str, &str)],
        new_fields: &[&FieldDefinition],
    ) -> Result<Vec<String>, AppError> {
        let mut statements = Vec::new();
        for (old_name, new_name) in renames {
            // Find the new field definition to get related_collection
            let new_field = new_fields.iter()
                .find(|f| f.name == *new_name)
                .ok_or_else(|| AppError::Internal(format!(
                    "Renamed field '{}' not found in new fields", new_name
                )))?;
            if new_field.field_type != FieldType::Relationship
                || new_field.relationship_type.as_deref() != Some("one_to_many")
            {
                continue;
            }
            let child_table = new_field.related_collection.as_ref()
                .ok_or_else(|| AppError::Internal(format!(
                    "1:M field '{}' missing related_collection", new_name
                )))?;

            let old_column = format!("{}_{}_id", source_table, old_name);
            let new_column = format!("{}_{}_id", source_table, new_name);
            let old_constraint = format!("fk_{}_{}_{}_id", child_table, source_table, old_name);
            let new_constraint = format!("fk_{}_{}_{}_id", child_table, source_table, new_name);

            // Drop old constraint
            statements.push(format!(
                "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {}",
                crate::db::quote_identifier(child_table),
                crate::db::quote_identifier(&old_constraint),
            ));
            // Rename column on child table
            statements.push(format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {}",
                crate::db::quote_identifier(child_table),
                crate::db::quote_identifier(&old_column),
                crate::db::quote_identifier(&new_column),
            ));
            // Add new constraint
            let fk = sea_query::ForeignKey::create()
                .name(&new_constraint)
                .from(sea_query::Alias::new(child_table), sea_query::Alias::new(&new_column))
                .to(sea_query::Alias::new(source_table), sea_query::Alias::new("id"))
                .on_delete(sea_query::ForeignKeyAction::Cascade)
                .on_update(sea_query::ForeignKeyAction::Cascade)
                .to_owned();
            statements.push(fk.to_string(sea_query::PostgresQueryBuilder));
        }
        Ok(statements)
    }

    /// Generate ALTER TABLE ADD CONSTRAINT FOREIGN KEY statements for all
    /// relationship fields in the given field list.
    ///
    /// For 1:1 relationships, also generates a UNIQUE constraint statement.
    /// Returns the FK SQL statement(s) ready to execute via sqlx.
    pub fn build_add_fk_constraint_sqls(
        table_name: &str,
        fields: &[&FieldDefinition],
    ) -> Result<Vec<String>, AppError> {
        let mut statements = Vec::new();

        for field in fields {
            if field.field_type != FieldType::Relationship {
                continue;
            }
            if Self::is_virtual_field(field) {
                continue; // 1:M FK handled by build_add_o2m_fk_sqls
            }
            let related_collection = field.related_collection.as_ref()
                .ok_or_else(|| AppError::Internal(format!(
                    "Relationship field '{}' missing related_collection", field.name
                )))?;
            let rel_type = field.relationship_type.as_deref()
                .unwrap_or("many_to_one");

            let constraint_name = format!("fk_{}_{}", table_name, field.name);

            // Build FK constraint via sea-query ForeignKey::create()
            let fk = ForeignKey::create()
                .name(&constraint_name)
                .from(Alias::new(table_name), Alias::new(&field.name))
                .to(Alias::new(related_collection), Alias::new("id"))
                .on_delete(ForeignKeyAction::Restrict)
                .on_update(ForeignKeyAction::Restrict)
                .to_owned();

            let fk_sql = fk.to_string(PostgresQueryBuilder);
            statements.push(fk_sql);

            // For 1:1, add UNIQUE constraint on the FK column
            if rel_type == "one_to_one" {
                let unique_name = format!("uq_{}_{}", table_name, field.name);
                let unique_sql = format!(
                    "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({})",
                    crate::db::quote_identifier(table_name),
                    crate::db::quote_identifier(&unique_name),
                    crate::db::quote_identifier(&field.name),
                );
                statements.push(unique_sql);
            }
        }

        Ok(statements)
    }

    /// Generate ALTER TABLE DROP CONSTRAINT statements for the FK constraint
    /// on each named field.
    ///
    /// Must be called BEFORE dropping the column itself (constraint must be
    /// removed before the column).
    pub fn build_drop_fk_constraint_sqls(
        table_name: &str,
        field_names: &[&str],
    ) -> Vec<String> {
        field_names.iter().map(|field_name| {
            let constraint_name = format!("fk_{}_{}", table_name, field_name);
            format!(
                "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {}",
                crate::db::quote_identifier(table_name),
                crate::db::quote_identifier(&constraint_name),
            )
        }).collect()
    }

    /// Generate ALTER TABLE statements for 1:M (one-to-many) relationship fields.
    ///
    /// For each 1:M field, generates SQL to:
    /// 1. Add a UUID FK column named `{source_table}_{field_name}_id` to the **target** collection's table
    /// 2. Add a FOREIGN KEY constraint with ON DELETE CASCADE
    ///
    /// Column naming uses both source_table and field_name to prevent collisions when
    /// multiple 1:M fields point to the same target collection.
    ///
    /// The `o2m_fields` slice must contain ONLY 1:M relationship fields (callers filter beforehand).
    pub fn build_add_o2m_fk_sqls(
        source_table: &str,
        o2m_fields: &[&FieldDefinition],
    ) -> Result<Vec<String>, AppError> {
        let mut statements = Vec::new();

        for field in o2m_fields {
            let child_table = field.related_collection.as_ref()
                .ok_or_else(|| AppError::Internal(format!(
                    "1:M field '{}' missing related_collection", field.name
                )))?;

            let fk_column = format!("{}_{}_id", source_table, field.name);
            let constraint_name = format!("fk_{}_{}_{}_id", child_table, source_table, field.name);

            // Add FK column to child table
            let add_col_sql = format!(
                "ALTER TABLE {} ADD COLUMN {} UUID",
                crate::db::quote_identifier(child_table),
                crate::db::quote_identifier(&fk_column),
            );
            statements.push(add_col_sql);

            // Add FK constraint with ON DELETE CASCADE
            let fk = sea_query::ForeignKey::create()
                .name(&constraint_name)
                .from(sea_query::Alias::new(child_table), sea_query::Alias::new(&fk_column))
                .to(sea_query::Alias::new(source_table), sea_query::Alias::new("id"))
                .on_delete(sea_query::ForeignKeyAction::Cascade)
                .on_update(sea_query::ForeignKeyAction::Cascade)
                .to_owned();

            let fk_sql = fk.to_string(sea_query::PostgresQueryBuilder);
            statements.push(fk_sql);
        }

        Ok(statements)
    }

    /// Generate ALTER TABLE statements to REMOVE 1:M FK columns + constraints.
    ///
    /// For each tuple (field_name, child_table_name), generates SQL to:
    /// 1. Drop the FK constraint on the target table
    /// 2. Drop the FK column on the target table
    pub fn build_drop_o2m_fk_sqls(
        source_table: &str,
        fields: &[(&str, &str)],
    ) -> Vec<String> {
        let mut statements = Vec::new();

        for (field_name, child_table) in fields {
            let fk_column = format!("{}_{}_id", source_table, field_name);
            let constraint_name = format!("fk_{}_{}_{}_id", child_table, source_table, field_name);

            // Drop FK constraint first (must come before DROP COLUMN)
            let drop_fk = format!(
                "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {}",
                crate::db::quote_identifier(child_table),
                crate::db::quote_identifier(&constraint_name),
            );
            statements.push(drop_fk);

            // Drop the FK column
            let drop_col = format!(
                "ALTER TABLE {} DROP COLUMN IF EXISTS {}",
                crate::db::quote_identifier(child_table),
                crate::db::quote_identifier(&fk_column),
            );
            statements.push(drop_col);
        }

        statements
    }

    /// Convert a FieldDefinition to a sea-query ColumnDef with proper type and constraints.
    ///
    /// Type mapping per COLL-07:
    /// - string → varchar(255)
    /// - text → text
    /// - int → integer
    /// - float → double precision
    /// - datetime → timestamptz
    /// - uuid → uuid
    ///
    /// Constraint mapping per COLL-08:
    /// - required=true → .not_null()
    /// - unique=true → .unique_key()
    /// - default=Some(...) → .default(Expr::...)
    fn field_to_column_def(field: &FieldDefinition) -> Result<ColumnDef, AppError> {
        if Self::is_virtual_field(field) {
            return Err(AppError::Internal(format!(
                "Cannot create column for virtual 1:M field '{}'", field.name
            )));
        }
        let mut col = ColumnDef::new(Alias::new(&field.name));

        // Set column type based on field type mapping
        match field.field_type {
            FieldType::String => col.string_len(255),
            FieldType::Text => col.text(),
            FieldType::Int => col.integer(),
            FieldType::Float => col.double(),
            FieldType::Datetime => col.timestamp_with_time_zone(),
            FieldType::Uuid => col.uuid(),
            // Relationship FK columns are UUID type (stores referenced collection item ID)
            FieldType::Relationship => col.uuid(),
            FieldType::Bool => col.boolean(),
            // File field stores UUID[] — array of file_metadata IDs
            FieldType::File => col.custom(sea_query::Alias::new("UUID[]")),
        };

        // Required → NOT NULL
        if field.required {
            col.not_null();
        }

        // Unique → UNIQUE
        if field.unique {
            col.unique_key();
        }

        // Default value → DEFAULT clause
        if let Some(ref default_val) = field.default {
            let expr = Self::build_default_expr(field, default_val)?;
            col.default(expr);
        }

        Ok(col)
    }

    /// Build a sea-query SimpleExpr for a field's default value.
    /// Uses Expr::cust() for type casts and Expr::value() for simple literals.
    fn build_default_expr(field: &FieldDefinition, default_val: &serde_json::Value) -> Result<SimpleExpr, AppError> {
        match (&field.field_type, default_val) {
            (FieldType::String | FieldType::Text, serde_json::Value::String(s)) => {
                Ok(Expr::value(s.clone()))
            }
            (FieldType::Int, serde_json::Value::Number(n)) => {
                if let Some(i) = n.as_i64() {
                    Ok(Expr::value(i as i32))
                } else {
                    Err(AppError::BadRequest(format!(
                        "Default value for int field '{}' must be an integer", field.name
                    )))
                }
            }
            (FieldType::Float, serde_json::Value::Number(n)) => {
                if let Some(f) = n.as_f64() {
                    Ok(Expr::value(f))
                } else {
                    Err(AppError::BadRequest(format!(
                        "Default value for float field '{}' must be a number", field.name
                    )))
                }
            }
            (FieldType::Datetime, serde_json::Value::String(s)) => {
                // Use cust() for ::timestamptz cast
                Ok(Expr::cust(format!("'{}'::timestamptz", s)))
            }
            (FieldType::Uuid, serde_json::Value::String(s)) => {
                Ok(Expr::cust(format!("'{}'::uuid", s)))
            }
            (FieldType::Bool, serde_json::Value::Bool(b)) => {
                Ok(Expr::value(*b))
            }
            (FieldType::Bool, serde_json::Value::String(s)) if s == "true" || s == "false" => {
                Ok(Expr::cust(s.clone()))
            }
            // Relationship fields do not support defaults (FK value must be set explicitly)
            (FieldType::Relationship, _) => Err(AppError::BadRequest(format!(
                "Relationship field '{}' does not support default values", field.name
            ))),
            // File fields do not support defaults (must be set explicitly)
            (FieldType::File, _) => Err(AppError::BadRequest(format!(
                "File field '{}' does not support default values", field.name
            ))),
            _ => Err(AppError::BadRequest(format!(
                "Invalid default value type for field '{}' of type {:?}",
                field.name, field.field_type
            ))),
        }
    }
}
