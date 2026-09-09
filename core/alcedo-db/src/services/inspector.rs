use serde::{Deserialize, Serialize};

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_common::state::{
    CoreAppVersion, CoreColumn, CoreDatabaseSchema, CoreForeignKey, CoreState, CoreTable,
};
use sqlx::postgres::PgRow;
use sqlx::Row;
use utoipa::ToSchema;

use crate::db::Pool;
use crate::services::tables::FieldSavedMetaObject;

/// Fetch raw rows for inspector queries directly from the core pool.
///
/// Prototype parity helper: the old `services::postgres::pool::execute_query`
/// took `&AppState`; it no longer exists, and `alcedo-db` must not depend
/// on the plugin-layer `AppState` (dependency cycle), so queries run
/// against [`CoreState::pool`] instead.
async fn fetch_rows(pool: &Pool, sql: &str) -> Result<Vec<PgRow>, sqlx::Error> {
    sqlx::query(sql).fetch_all(pool).await
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DatabaseSchema {
    pub app_versions: Vec<AppVersion>,
    pub tables: Vec<Table>,
    pub columns: Vec<Column>,
}

impl DatabaseSchema {
    pub fn new() -> DatabaseSchema {
        return DatabaseSchema {
            app_versions: vec![],
            columns: vec![],
            tables: vec![],
        };
    }

    pub async fn refresh(&self, core: &CoreState) -> Self {
        let pool = core
            .pool
            .as_ref()
            .expect("DatabaseSchema::refresh requires a configured pool");
        let mut schema = DatabaseSchema {
            app_versions: vec![],
            tables: vec![],
            columns: vec![],
        };

        let alcedo_tables_exist: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind("alcedo.alcedo_apps_versions")
            .fetch_one(pool)
            .await
            .unwrap();

        if alcedo_tables_exist {
            let fetch_versions = fetch_rows(
            pool,
            "select app_id,version_id, name,api_name,version_name from alcedo.alcedo_apps_versions
join alcedo.alcedo_apps on alcedo.alcedo_apps.id = alcedo.alcedo_apps_versions.app_id
join alcedo.alcedo_versions on alcedo.alcedo_versions.id = alcedo.alcedo_apps_versions.version_id",
        )
        .await;

            for version in fetch_versions.unwrap() {
                // println!("{:?}",version);
                schema.app_versions.push(AppVersion {
                    version_id: version.get("version_id"),
                    version_name: version.get("version_name"),
                    app_id: version.get("app_id"),
                    app_name: version.get("api_name"),
                    schema_name: AppContext {
                        app_name: version.get("api_name"),
                        version: version.get("version_name"),
                        request_source: RequestSource::Inspector,
                    }
                    .schema_name(),
                })
            }
        }
        // println!("{:?}",schema.app_versions);

        let fetch_tables = fetch_rows(
            pool,
            "select * from information_schema.tables 
  WHERE (table_schema like '%010%' or table_schema = 'alcedo') and table_type = 'BASE TABLE'",
        )
        .await;

        for table in fetch_tables.unwrap() {
            let name = table.try_get("table_name").unwrap();
            let schema_name = table.try_get("table_schema").unwrap();

            schema.tables.push(Table {
                name,
                schema: schema_name,
                meta: None,
            })
        }

        let fetch_columns = fetch_rows(
            pool,
            "SELECT *
FROM information_schema.columns
WHERE (table_schema like '%010%' or table_schema = 'alcedo')
",
        )
        .await
        .unwrap();

        for column in fetch_columns {
            let name = column.try_get("column_name").unwrap();
            let table = column.try_get("table_name").unwrap();
            let schema_name = column.try_get("table_schema").unwrap();
            let data_type = column.try_get("data_type").unwrap();
            let default_value: Option<String> =
                extract_default(column.try_get("column_default").unwrap());
            let numeric_precision = column.try_get("numeric_precision").unwrap();
            let numeric_scale = column.try_get("numeric_scale").unwrap();
            let max_length = column.try_get("character_maximum_length").unwrap();
            let is_nullable = column.try_get::<String, _>("is_nullable").unwrap() == "YES";
            let generated = column.try_get::<String, _>("is_generated").unwrap() != "ALWAYS";
            let generation_expression = column.try_get("generation_expression").unwrap();
            let has_auto_increment = if let Some(dv) = &default_value {
                format!("{}", dv).starts_with("nextval(")
            } else {
                false
            };

            schema.columns.push(Column {
                schema: schema_name,
                name,
                table,
                data_type,
                default_value,
                max_length,
                numeric_precision,
                numeric_scale,
                is_nullable,
                is_unique: false,
                is_indexed: false,
                is_primary_key: false,
                generated,
                generation_expression,
                has_auto_increment,
                foreign_key: None,
                meta: None,
            })
        }

        let fetch_indexes = fetch_rows(
            pool,
            "SELECT * FROM
    pg_indexes
",
        )
        .await
        .unwrap();

        for row in fetch_indexes {
            let tablename: String = row.try_get("tablename").unwrap();
            let schemaname: String = row.try_get("schemaname").unwrap();
            let indexdef: Option<String> = row.try_get("indexdef").unwrap();

            let indexdef = match indexdef {
                Some(def) => def,
                None => continue,
            };

            // Skip if the index definition doesn't contain fields
            let fields_part = match indexdef.split_once('(') {
                Some((_, rest)) => rest.strip_suffix(')').unwrap_or(rest),
                None => continue,
            };

            for field_raw in fields_part.split(',') {
                let field = field_raw.trim();

                if let Some(col) = schema
                    .columns
                    .iter_mut()
                    .find(|c| c.schema == schemaname && c.name == field && c.table == tablename)
                {
                    col.is_unique = indexdef.contains("UNIQUE");
                    col.is_indexed = true;
                }
            }
        }

        let fetch_fks = fetch_rows(
            pool,
            "SELECT
    tc.table_schema        AS table_schema,
    tc.table_name          AS fk_table,
    kcu.column_name        AS fk_column,
    ccu.table_schema       AS referenced_schema,
    ccu.table_name         AS referenced_table,
    ccu.column_name        AS referenced_column
FROM information_schema.table_constraints tc
JOIN information_schema.key_column_usage kcu
    ON tc.constraint_name = kcu.constraint_name
   AND tc.constraint_schema = kcu.constraint_schema
JOIN information_schema.constraint_column_usage ccu
    ON tc.constraint_name = ccu.constraint_name
   AND tc.constraint_schema = ccu.constraint_schema
WHERE tc.constraint_type = 'FOREIGN KEY'
ORDER BY
    tc.table_schema,
    tc.table_name,
    kcu.column_name;
",
        )
        .await
        .unwrap();

        for row in fetch_fks {
            let fk_table: String = row.try_get("fk_table").unwrap();
            let fk_column: String = row.try_get("fk_column").unwrap();
            let referenced_table = row.try_get("referenced_table").unwrap();
            let referenced_column = row.try_get("referenced_column").unwrap();
            let table_schema: String = row.try_get("table_schema").unwrap();
            let referenced_schema = row.try_get("referenced_schema").unwrap();

            if let Some(col) = schema
                .columns
                .iter_mut()
                .find(|c| c.schema == table_schema && c.name == fk_column && c.table == fk_table)
            {
                col.foreign_key = Some(ForeignKey {
                    table: referenced_table,
                    column: referenced_column,
                    schema: referenced_schema,
                })
            }
        }

        let fetch_pks = fetch_rows(
            pool,
            "SELECT
    tc.table_schema as table_schema,
    tc.table_name as tablename,
    kcu.column_name as column
FROM information_schema.table_constraints AS tc
JOIN information_schema.key_column_usage AS kcu
    ON tc.constraint_name = kcu.constraint_name
   AND tc.table_schema = kcu.table_schema
WHERE tc.constraint_type = 'PRIMARY KEY'
ORDER BY tc.table_schema, tc.table_name, kcu.ordinal_position;
",
        )
        .await
        .unwrap();

        for row in fetch_pks {
            let table_schema: String = row.try_get("table_schema").unwrap();
            let tablename: String = row.try_get("tablename").unwrap();
            let column: Option<String> = row.try_get("column").unwrap();

            let column = match column {
                Some(def) => def,
                None => continue,
            };

            if let Some(col) = schema
                .columns
                .iter_mut()
                .find(|c| c.schema == table_schema && c.name == column && c.table == tablename)
            {
                col.is_primary_key = true;
            }
        }

        schema
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppVersion {
    pub version_id: i32,
    pub app_id: i32,
    pub schema_name: String,
    pub app_name: String,
    pub version_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct TableMeta {
    pub id: Option<i64>,
    pub app_name: String,
    pub app_version: String,
    pub table: String,
    pub name: String,
    pub icon_name: Option<String>,
    pub icon_color: Option<String>,
    pub singleton: bool,
    pub hidden: bool,
    pub sort_field: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Table {
    pub name: String,
    pub schema: String,
    pub meta: Option<TableMeta>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Column {
    pub schema: String,
    pub table: String,

    pub name: String,
    pub data_type: String,
    pub default_value: Option<String>,

    pub max_length: Option<i32>,
    pub numeric_precision: Option<i32>,
    pub numeric_scale: Option<i32>,

    pub is_nullable: bool,
    pub is_unique: bool,
    pub is_indexed: bool,
    pub is_primary_key: bool,

    pub generated: bool,
    pub generation_expression: Option<String>,

    pub has_auto_increment: bool,
    pub foreign_key: Option<ForeignKey>,

    pub meta: Option<FieldSavedMetaObject>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct ForeignKey {
    pub table: String,
    pub column: String,
    pub schema: String,
}

fn extract_default(default_str: Option<&str>) -> Option<String> {
    let default_str = match default_str {
        Some(s) if !s.is_empty() && s.to_uppercase() != "NULL" => s,
        _ => return None,
    };

    if default_str.starts_with('\'') {
        if let Some(end_quote) = find_closing_quote(default_str) {
            let value = &default_str[1..end_quote];

            return Some(value.replace("''", "'"));
        }
    }

    if default_str.contains('(') {
        return Some(default_str.to_string());
    }

    if let Some(pos) = default_str.find("::") {
        return Some(default_str[..pos].to_string());
    }

    Some(default_str.to_string())
}

fn find_closing_quote(s: &str) -> Option<usize> {
    let mut chars = s.char_indices().skip(1);
    while let Some((i, ch)) = chars.next() {
        if ch == '\'' {
            if let Some((_, next_ch)) = chars.clone().next() {
                if next_ch == '\'' {
                    chars.next();
                    continue;
                }
            }
            return Some(i);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Conversions into the `alcedo-common` duplicate schema types
// ---------------------------------------------------------------------------
// `CoreState` (leaf crate) cannot name these db-side types, so the
// conversion lives here. `meta` round-trips through `serde_json::Value`.

impl From<AppVersion> for CoreAppVersion {
    fn from(v: AppVersion) -> Self {
        Self {
            version_id: v.version_id,
            app_id: v.app_id,
            schema_name: v.schema_name,
            app_name: v.app_name,
            version_name: v.version_name,
        }
    }
}

impl From<ForeignKey> for CoreForeignKey {
    fn from(v: ForeignKey) -> Self {
        Self {
            table: v.table,
            column: v.column,
            schema: v.schema,
        }
    }
}

impl From<Table> for CoreTable {
    fn from(v: Table) -> Self {
        Self {
            name: v.name,
            schema: v.schema,
            meta: v.meta.and_then(|m| serde_json::to_value(m).ok()),
        }
    }
}

impl From<Column> for CoreColumn {
    fn from(v: Column) -> Self {
        Self {
            schema: v.schema,
            table: v.table,
            name: v.name,
            data_type: v.data_type,
            default_value: v.default_value,
            max_length: v.max_length,
            numeric_precision: v.numeric_precision,
            numeric_scale: v.numeric_scale,
            is_nullable: v.is_nullable,
            is_unique: v.is_unique,
            is_indexed: v.is_indexed,
            is_primary_key: v.is_primary_key,
            generated: v.generated,
            generation_expression: v.generation_expression,
            has_auto_increment: v.has_auto_increment,
            foreign_key: v.foreign_key.map(CoreForeignKey::from),
            meta: v.meta.and_then(|m| serde_json::to_value(m).ok()),
        }
    }
}

impl From<DatabaseSchema> for CoreDatabaseSchema {
    fn from(v: DatabaseSchema) -> Self {
        Self {
            app_versions: v.app_versions.into_iter().map(CoreAppVersion::from).collect(),
            tables: v.tables.into_iter().map(CoreTable::from).collect(),
            columns: v.columns.into_iter().map(CoreColumn::from).collect(),
        }
    }
}
