use serde::{Deserialize, Serialize};

use sqlx::Row;
use sqlx::postgres::PgRow;
use utoipa::ToSchema;

use crate::{
    AppState,
    services::{
        collections::{FieldDefinition, schema::FieldSavedMetaObject},
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        postgres::pool::execute_query,
    },
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DatabaseSchema {
    pub app_versions: Vec<AppVersion>,
    pub tables: Vec<Table>,
    pub columns: Vec<Column>,
    #[serde(default)]
    pub fields: Vec<TableFields>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableFields {
    pub schema: String,
    pub table: String,
    pub fields: Vec<FieldDefinition>,
}

/// Runs an introspection query, retrying on transient catalog errors: a
/// concurrent DROP SCHEMA/TABLE can make catalog views fail with
/// "could not open relation with OID" on a stale snapshot; a fresh snapshot
/// resolves it.
async fn execute_introspection_query(
    app_state: &AppState,
    sql: String,
) -> Result<Vec<PgRow>, AlcedoError> {
    let mut last_err = None;
    for _ in 0..3 {
        match execute_query(app_state, sql.clone()).await {
            Ok(rows) => return Ok(rows),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
    Err(last_err.unwrap())
}

impl DatabaseSchema {
    pub fn new() -> DatabaseSchema {
        return DatabaseSchema {
            app_versions: vec![],
            columns: vec![],
            tables: vec![],
            fields: vec![],
        };
    }

    pub fn collection_id(&self, schema: &str, table: &str) -> Option<i64> {
        self.tables
            .iter()
            .find(|t| t.schema == schema && t.name == table)
            .and_then(|t| t.meta.as_ref())
            .and_then(|m| m.id)
    }

    pub fn collection_table(&self, schema: &str, id: i64) -> Option<String> {
        self.tables
            .iter()
            .find(|t| t.schema == schema && t.meta.as_ref().and_then(|m| m.id) == Some(id))
            .map(|t| t.name.clone())
    }

    pub async fn refresh(app_state: &AppState) -> Result<Self, AlcedoError> {
        Self::refresh_inner(app_state, None).await
    }

    /// The app-version link list (cheap 3-table join). Scoped refreshes
    /// re-run this so the cache's `app_versions` stays current when links
    /// change (e.g. a deploy linking a new app to production).
    /// Returns `None` on any error so callers keep the cached list instead
    /// of wiping it with an empty one.
    pub async fn fetch_app_versions(app_state: &AppState) -> Option<Vec<AppVersion>> {
        let alcedocore_tables_exist: bool = match sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
                .bind("alcedocore.alcedocore_apps_versions")
                .fetch_one(&*app_state.database_pool)
                .await
            {
                Ok(v) => v,
                Err(_) => return None,
            };
        if !alcedocore_tables_exist {
            return Some(vec![]);
        }
        let fetch_versions = match execute_introspection_query(
            app_state,
            "select app_id,version_id, name,api_name,version_name from alcedocore.alcedocore_apps_versions
join alcedocore.alcedocore_apps on alcedocore.alcedocore_apps.id = alcedocore.alcedocore_apps_versions.app_id
join alcedocore.alcedocore_versions on alcedocore.alcedocore_versions.id = alcedocore.alcedocore_apps_versions.version_id"
                .to_string(),
        )
        .await
        {
            Ok(rows) => rows,
            Err(_) => return None,
        };

        Some(
            fetch_versions
                .into_iter()
                .map(|version| AppVersion {
                    version_id: version.get("version_id"),
                    version_name: version.get("version_name"),
                    app_id: version.get("app_id"),
                    app_name: version.get("api_name"),
                    schema_name: AppContext {
                        app_name: version.get("api_name"),
                        version: version.get("version_name"),
                        request_source: RequestSource::Inspector,
                        identity: None,
                        request_id: None,
                    }
                    .schema_name(),
                })
                .collect(),
        )
    }

    /// Introspects only `schema_name` (plus the global `alcedocore` schema,
    /// which the synthetic users table is cloned from). App-scoped DDL only
    /// ever touches the request's own schema, so app-scoped refreshes use
    /// this instead of scanning every app-version schema on every save.
    pub async fn refresh_scoped(app_state: &AppState, schema_name: &str) -> Result<Self, AlcedoError> {
        Self::refresh_inner(app_state, Some(schema_name)).await
    }

    async fn refresh_inner(app_state: &AppState, only_schema: Option<&str>) -> Result<Self, AlcedoError> {
        let mut schema = DatabaseSchema {
            app_versions: vec![],
            tables: vec![],
            columns: vec![],
            fields: vec![],
        };

        // Schema predicates: full refresh scans every app-version schema;
        // scoped refresh touches only the target schema + alcedocore.
        // Deploy backups (`{app}010backup_before_deploy_*`) are excluded from
        // the full scan: they are only ever read back via raw SQL during a
        // rollback, never served, and every deploy adds one — including them
        // made the introspection grow (and the pool starve) with each deploy.
        let esc = |s: &str| s.replace('\'', "''");
        let table_pred = match only_schema {
            Some(s) => format!("(table_schema = '{}' OR table_schema = 'alcedocore')", esc(s)),
            None => "(table_schema = 'alcedocore' OR (table_schema like '%010%' AND table_schema NOT LIKE '%010backup_before_deploy_%'))".to_string(),
        };
        let index_pred = match only_schema {
            Some(s) => format!("(schemaname = '{}' OR schemaname = 'alcedocore')", esc(s)),
            None => "(schemaname = 'alcedocore' OR (schemaname like '%010%' AND schemaname NOT LIKE '%010backup_before_deploy_%'))".to_string(),
        };
        let fk_pred = match only_schema {
            Some(s) => format!("(n.nspname = '{}' OR n.nspname = 'alcedocore')", esc(s)),
            None => "(n.nspname = 'alcedocore' OR (n.nspname like '%010%' AND n.nspname NOT LIKE '%010backup_before_deploy_%'))".to_string(),
        };

        if let Some(s) = only_schema {
            schema.app_versions.push(AppVersion {
                version_id: 0,
                app_id: 0,
                schema_name: s.to_string(),
                app_name: s.to_string(),
                version_name: String::new(),
            });
        }

        if only_schema.is_none() {
            if let Some(versions) = Self::fetch_app_versions(app_state).await {
                schema.app_versions = versions;
            }
        }
        // println!("{:?}",schema.app_versions);

        let tables_sql = format!(
            "select * from information_schema.tables 
  WHERE {} and table_type = 'BASE TABLE'",
            table_pred
        );

        let columns_sql = format!(
            "SELECT *
FROM information_schema.columns
WHERE {}
",
            table_pred
        );

        let indexes_sql = format!(
            "SELECT * FROM
    pg_indexes
    WHERE {}
",
            index_pred
        );

        let fks_sql = format!(
            "SELECT
    n.nspname  AS table_schema,
    cl.relname AS fk_table,
    a.attname  AS fk_column,
    rn.nspname AS referenced_schema,
    rc.relname AS referenced_table,
    ra.attname AS referenced_column
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class cl ON cl.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = cl.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = cl.oid AND a.attnum = ANY(con.conkey)
LEFT JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid
LEFT JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace
LEFT JOIN pg_catalog.pg_attribute ra ON ra.attrelid = rc.oid AND ra.attnum = ANY(con.confkey)
WHERE con.contype = 'f'
  AND {}
ORDER BY
    n.nspname,
    cl.relname,
    a.attname;
",
                fk_pred
        );

        let pks_sql = format!(
            "SELECT
    n.nspname  AS table_schema,
    cl.relname AS tablename,
    a.attname  AS column
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class cl ON cl.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = cl.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = cl.oid AND a.attnum = ANY(con.conkey)
WHERE con.contype = 'p'
  AND {}
ORDER BY n.nspname, cl.relname, a.attnum;
",
            fk_pred
        );

        // The five catalog queries are independent — run them concurrently on
        // separate pool connections instead of five sequential round trips.
        let (fetch_tables, fetch_columns, fetch_indexes, fetch_fks, fetch_pks) = tokio::join!(
            execute_introspection_query(&app_state, tables_sql),
            execute_introspection_query(&app_state, columns_sql),
            execute_introspection_query(&app_state, indexes_sql),
            execute_introspection_query(&app_state, fks_sql),
            execute_introspection_query(&app_state, pks_sql),
        );
        let fetch_tables = fetch_tables?;
        let fetch_columns = fetch_columns?;
        let fetch_indexes = fetch_indexes?;
        let fetch_fks = fetch_fks?;
        let fetch_pks = fetch_pks?;


        for table in &fetch_tables {
            let name = table.try_get("table_name").unwrap();
            let schema_name = table.try_get("table_schema").unwrap();

            schema.tables.push(Table {
                name,
                schema: schema_name,
                meta: None,
            })
        }


        for column in fetch_columns {
            let name = column.try_get("column_name").unwrap();
            let table = column.try_get("table_name").unwrap();
            let schema_name = column.try_get("table_schema").unwrap();
            let data_type = column.try_get("data_type").unwrap();
            let udt_name: String = column.try_get("udt_name").unwrap();
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
                udt_name,
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

        // Register the global users table as a collection in every app schema so
        // it can be a policy target and be referenced by `user` fields. The
        // physical table lives in the `alcedocore` schema; the synthetic entry uses
        // the reserved collection id `-1`.
        let users_columns: Vec<Column> = schema
            .columns
            .iter()
            .filter(|c| c.schema == "alcedocore" && c.table == "alcedocore_users")
            .cloned()
            .collect();
        if !users_columns.is_empty() {
            for version in schema.app_versions.clone() {
                let app_schema = version.schema_name.clone();
                if schema
                    .tables
                    .iter()
                    .any(|t| t.schema == app_schema && t.name == "alcedocore_users")
                {
                    continue;
                }
                schema.tables.push(Table {
                    name: "alcedocore_users".to_string(),
                    schema: app_schema.clone(),
                    // Meta is attached from the seeded `alcedocore_collections` row
                    // by `refresh_meta`.
                    meta: None,
                });
                for column in &users_columns {
                    let mut synthetic = column.clone();
                    synthetic.schema = app_schema.clone();
                    schema.columns.push(synthetic);
                }
            }
        }

        Ok(schema)
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
    /// Postgres `udt_name` (e.g. `_uuid` for `uuid[]`); used to type array columns.
    #[serde(default)]
    pub udt_name: String,
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
