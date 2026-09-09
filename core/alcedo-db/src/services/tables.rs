use sea_query::{
    Alias, ColumnDef, Expr, ForeignKey, Index, PostgresQueryBuilder, Table, TableAlterStatement,
    TableCreateStatement, TableForeignKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{Postgres, Transaction};
use tokio::sync::RwLock;
use utoipa::ToSchema;

use alcedo_common::{
    context::{AppContext, RequestSource},
    error::AppError,
    state::{CoreColumn, CoreDatabaseSchema, CoreState},
};

use crate::db::Pool;
use crate::services::inspector::{DatabaseSchema, ForeignKey as InspectorForeignKey, TableMeta};

// ---------------------------------------------------------------------------
// Local SQL helpers
// ---------------------------------------------------------------------------
// The prototype `TableService` relied on `services::items::ItemsService`
// and `services::postgres::pool::{execute_query, execute_query_transaction}`,
// which were never ported to the split-crate layout (and `ItemsService`
// would pull the plugin-layer `AppState` back in — the dependency cycle
// being fixed here). The meta-table operations below are all simple CRUD
// on `alcedo_collections` / `alcedo_fields`, implemented directly with
// sqlx so `alcedo-db` only depends on [`CoreState`].

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn schema_table(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(table))
}

async fn exec_tx(tx: &mut Transaction<'_, Postgres>, sql: &str) -> Result<(), AppError> {
    sqlx::query(sql).execute(&mut **tx).await?;
    Ok(())
}

type PgQuery<'q> = sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>;

/// Bind a `serde_json::Value` with best-effort typing. Nested values are
/// stored as JSON text (works for `jsonb` columns).
fn bind_value<'q>(q: PgQuery<'q>, v: &'q Value) -> PgQuery<'q> {
    match v {
        Value::Null => q.bind(None::<String>),
        Value::Bool(b) => q.bind(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                q.bind(i)
            } else if let Some(u) = n.as_u64() {
                // Meta ids always fit in i64; saturate defensively.
                q.bind(u.min(i64::MAX as u64) as i64)
            } else if let Some(f) = n.as_f64() {
                q.bind(f)
            } else {
                q.bind(n.to_string())
            }
        }
        Value::String(s) => q.bind(s.clone()),
        Value::Array(_) | Value::Object(_) => q.bind(v.to_string()),
    }
}

async fn meta_insert(
    tx: &mut Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
    row: &Map<String, Value>,
) -> Result<(), AppError> {
    if row.is_empty() {
        return Err(AppError::BadRequest(
            "Cannot insert an empty meta row".to_string(),
        ));
    }
    let cols: Vec<&String> = row.keys().collect();
    let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("${}", i)).collect();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        schema_table(schema, table),
        cols.iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", "),
        placeholders.join(", "),
    );
    let mut q = sqlx::query(&sql);
    for v in row.values() {
        q = bind_value(q, v);
    }
    q.execute(&mut **tx).await?;
    Ok(())
}

async fn meta_select_all(
    pool: &Pool,
    schema: &str,
    table: &str,
) -> Result<Vec<Map<String, Value>>, AppError> {
    let sql = format!(
        "SELECT row_to_json(t.*) AS row FROM {} t",
        schema_table(schema, table)
    );
    let rows: Vec<(Value,)> = sqlx::query_as(&sql).fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .filter_map(|(v,)| v.as_object().cloned())
        .collect())
}

async fn meta_delete_by_id(
    tx: &mut Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
    id: i64,
) -> Result<(), AppError> {
    let sql = format!(
        "DELETE FROM {} WHERE \"id\" = $1",
        schema_table(schema, table)
    );
    sqlx::query(&sql).bind(id).execute(&mut **tx).await?;
    Ok(())
}

async fn meta_delete_where_collection_id(
    tx: &mut Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
    collection_id: i64,
) -> Result<(), AppError> {
    let sql = format!(
        "DELETE FROM {} WHERE \"collection_id\" = $1",
        schema_table(schema, table)
    );
    sqlx::query(&sql)
        .bind(collection_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn meta_update_by_id(
    tx: &mut Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
    id: i64,
    patch: &Map<String, Value>,
) -> Result<(), AppError> {
    if patch.is_empty() {
        return Ok(());
    }
    let keys: Vec<&String> = patch.keys().collect();
    let set_clause = keys
        .iter()
        .enumerate()
        .map(|(i, k)| format!("{} = ${}", quote_ident(k), i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "UPDATE {} SET {} WHERE \"id\" = ${}",
        schema_table(schema, table),
        set_clause,
        keys.len() + 1
    );
    let mut q = sqlx::query(&sql);
    for k in &keys {
        q = bind_value(q, &patch[*k]);
    }
    q = q.bind(id);
    q.execute(&mut **tx).await?;
    Ok(())
}

/// Extract a numeric `id` from a meta row stored as `serde_json::Value`.
/// (Covers both `TableMeta.id: Option<i64>` and
/// `FieldSavedMetaObject.id: i32`, which serialize as JSON numbers.)
fn meta_id(meta: &Value) -> Option<i64> {
    meta.get("id")?.as_i64()
}

pub trait TableBuilderExt {
    fn add_col(&mut self, name: &str, f: impl FnOnce(ColumnDef) -> ColumnDef) -> &mut Self;
    fn add_fk(
        &mut self,
        from_context: &AppContext,
        from_table: &str,
        from_col: &str,
        to_context: &AppContext,
        to_table: &str,
        to_col: &str,
    ) -> &mut Self;
}

impl TableBuilderExt for TableCreateStatement {
    fn add_col(&mut self, name: &str, f: impl FnOnce(ColumnDef) -> ColumnDef) -> &mut Self {
        self.col(f(ColumnDef::new(Alias::new(name))));
        self
    }

    fn add_fk(
        &mut self,
        from_context: &AppContext,
        from_table: &str,
        from_col: &str,
        to_context: &AppContext,
        to_table: &str,
        to_col: &str,
    ) -> &mut Self {
        self.foreign_key(
            ForeignKey::create()
                .name(format!(
                    "{}_{}_{}_{}",
                    from_table, from_col, to_table, to_col
                ))
                .from(
                    (
                        Alias::new(from_context.schema_name()),
                        Alias::new(from_table),
                    ),
                    Alias::new(from_col),
                )
                .to(
                    (Alias::new(to_context.schema_name()), Alias::new(to_table)),
                    Alias::new(to_col),
                ),
        );
        self
    }
}

impl TableBuilderExt for TableAlterStatement {
    fn add_col(&mut self, name: &str, f: impl FnOnce(ColumnDef) -> ColumnDef) -> &mut Self {
        self.add_column(f(ColumnDef::new(Alias::new(name))));
        self
    }

    fn add_fk(
        &mut self,
        from_context: &AppContext,
        from_table: &str,
        from_col: &str,
        to_context: &AppContext,
        to_table: &str,
        to_col: &str,
    ) -> &mut Self {
        self.add_foreign_key(
            TableForeignKey::new()
                .name(format!(
                    "{}_{}_{}_{}",
                    from_table, from_col, to_table, to_col
                ))
                .from_tbl((
                    Alias::new(from_context.schema_name()),
                    Alias::new(from_table),
                ))
                .from_col(Alias::new(from_col))
                .to_tbl((Alias::new(to_context.schema_name()), Alias::new(to_table)))
                .to_col(Alias::new(to_col)),
        );
        self
    }
}

#[derive(ToSchema, Serialize, Default, Deserialize)]
pub struct FieldCreationObject {
    pub name: String,
    #[serde(rename = "type")]
    pub col_type: String,
    pub default_value: Option<String>,
    pub max_length: Option<u32>,
    pub numeric_precision: Option<u32>,
    pub numeric_scale: Option<u32>,
    pub is_nullable: Option<bool>,
    pub is_unique: Option<bool>,
    pub has_auto_increment: Option<bool>,
    pub foreign_key: Option<InspectorForeignKey>,
}
#[derive(ToSchema, Serialize, Default, Deserialize)]
pub struct FieldUpdateObject {
    pub name: Option<String>,
    pub default_value: Option<String>,
    pub max_length: Option<u32>,
    pub numeric_precision: Option<u32>,
    pub numeric_scale: Option<u32>,
    pub is_nullable: Option<bool>,
    pub is_unique: Option<bool>,
}

#[derive(ToSchema, Serialize, Default, Deserialize, Debug, Clone)]
pub struct FieldSavedMetaObject {
    pub api_name: String,
    pub display_name: String,
    pub collection_id: Option<i32>,
    pub id: i32,
    pub options: Option<serde_json::Value>,
}

#[derive(ToSchema, Serialize, Default, Deserialize, Debug, Clone)]
pub struct FieldMetaObject {
    pub api_name: String,
    pub display_name: String,
    pub collection_id: Option<i64>,
    pub options: Option<serde_json::Value>,
}

pub struct TableService<'a> {
    core: &'a CoreState,
    app_context: &'a AppContext,
}

impl TableService<'_> {
    pub fn new<'a>(core: &'a CoreState, context: &'a AppContext) -> TableService<'a> {
        return TableService {
            core,
            app_context: context,
        };
    }

    fn pool(&self) -> Result<&Pool, AppError> {
        self.core.pool()
    }

    pub async fn refresh_schema(&self) {
        let fresh = DatabaseSchema::new().refresh(self.core).await;
        *self.core.schema.write().await = CoreDatabaseSchema::from(fresh);

        if self.app_context.app_name == "alcedo" {
            return;
        }

        match self.app_context.request_source {
            RequestSource::FirstMigration => (),
            _ => self.refresh_meta().await,
        };
    }

    pub async fn refresh_meta(&self) {
        let schema_name = self.app_context.schema_name();
        let pool = self
            .pool()
            .expect("refresh_meta requires a configured pool");

        let meta_collections = meta_select_all(pool, &schema_name, "alcedo_collections")
            .await
            .unwrap();
        let meta_fields = meta_select_all(pool, &schema_name, "alcedo_fields")
            .await
            .unwrap();

        let mut write_schema_lock = self.core.schema.write().await;

        for table in write_schema_lock.tables.clone() {
            let find_meta = meta_collections.iter().find(|col| {
                format!(
                    "{}010{}",
                    col.get("app_name").unwrap().as_str().unwrap().to_string(),
                    col.get("app_version")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string()
                ) == table.schema
                    && col.get("table").unwrap().as_str().unwrap().to_string() == table.name
            });

            if let Some(meta) = find_meta {
                let meta_value = Value::Object(meta.clone());
                let table_meta: TableMeta = serde_json::from_value(meta_value.clone())
                    .expect("Invalid table meta row in alcedo_collections");

                {
                    let find_table = write_schema_lock
                        .tables
                        .iter_mut()
                        .find(|t| t.schema == table.schema && table_meta.table == t.name);

                    if let Some(find_table) = find_table {
                        find_table.meta = Some(meta_value);
                    }
                }

                let collection_id = table_meta.id.expect("Table meta row missing id");

                let fields: Vec<_> = meta_fields
                    .iter()
                    .filter(|field| {
                        field.get("collection_id").and_then(|v| v.as_i64()) == Some(collection_id)
                    })
                    .collect();

                for field in fields {
                    let field_meta: FieldSavedMetaObject =
                        serde_json::from_value(Value::Object(field.clone()))
                            .expect("Invalid field meta row in alcedo_fields");

                    let find = write_schema_lock.columns.iter_mut().find(|col| {
                        col.name == field_meta.api_name
                            && col.table == table.name
                            && col.schema == table.schema
                    });

                    if let Some(col) = find {
                        col.meta = Some(
                            serde_json::to_value(&field_meta).expect("Field meta must serialize"),
                        );
                    }
                }
            }
        }
    }

    pub async fn create_table<F>(
        &self,
        name: &str,
        builder: F,
        meta: Option<TableMeta>,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AppError>
    where
        F: FnOnce(&mut TableCreateStatement),
    {
        let pool = self.pool()?.clone();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref: &mut Transaction<'_, Postgres> = match database_transaction {
            Some(existing) => &mut **existing,
            None => {
                owned_tx = Some(pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };
        if let Some(meta) = meta {
            let meta: Map<String, Value> = match serde_json::to_value(&meta) {
                Err(_) => {
                    return Err(AppError::BadRequest("Invalid table meta".to_string()));
                }
                Ok(meta) => meta
                    .as_object()
                    .cloned()
                    .ok_or_else(|| AppError::BadRequest("Invalid table meta".to_string()))?,
            };
            meta_insert(
                tx_ref,
                &self.app_context.schema_name(),
                "alcedo_collections",
                &meta,
            )
            .await?;
        }

        let sql = {
            let mut stmt = Table::create();

            stmt.table((Alias::new(self.app_context.schema_name()), Alias::new(name)));
            builder(&mut stmt);
            stmt.to_string(PostgresQueryBuilder)
        };
        exec_tx(tx_ref, &sql).await?;

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };

        self.refresh_schema().await;

        Ok(())
    }

    pub async fn drop_table(
        &self,
        name: &str,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AppError> {
        let pool = self.pool()?.clone();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref: &mut Transaction<'_, Postgres> = match database_transaction {
            Some(existing) => &mut **existing,
            None => {
                owned_tx = Some(pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };
        let read_guard = self.core.schema.read().await;

        let find_table = read_guard
            .tables
            .iter()
            .find(|table| table.schema == self.app_context.schema_name() && table.name == name);

        if let None = find_table {
            return Err(AppError::NotFound(format!(
                "The table {} could not be found.",
                name
            )));
        }

        let find_table = find_table.unwrap();

        if let Some(meta) = &find_table.meta {
            if let Some(meta_id) = meta_id(meta) {
                meta_delete_where_collection_id(
                    tx_ref,
                    &self.app_context.schema_name(),
                    "alcedo_fields",
                    meta_id,
                )
                .await?;

                meta_delete_by_id(
                    tx_ref,
                    &self.app_context.schema_name(),
                    "alcedo_collections",
                    meta_id,
                )
                .await?;
            }
        }
        drop(read_guard);

        let sql = {
            let stmt = Table::drop()
                .table((Alias::new(self.app_context.schema_name()), Alias::new(name)))
                .if_exists()
                .to_owned();
            stmt.to_string(PostgresQueryBuilder)
        };

        exec_tx(tx_ref, &sql).await?;

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };

        self.refresh_schema().await;
        Ok(())
    }

    pub async fn add_field(
        &self,
        table: &str,
        field: FieldCreationObject,
        meta: Option<FieldMetaObject>,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AppError> {
        let pool = self.pool()?.clone();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref: &mut Transaction<'_, Postgres> = match database_transaction {
            Some(existing) => &mut **existing,
            None => {
                owned_tx = Some(pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };
        if let Some(meta) = meta {
            let collection_id = {
                let read_guard = self.core.schema.read().await;
                let find_collection = read_guard.tables.iter().find(|collection| {
                    collection.schema == self.app_context.schema_name() && collection.name == table
                });

                if let None = find_collection {
                    return Err(AppError::NotFound("Collection not found!".to_string()));
                };
                let find_collection = find_collection.unwrap();
                match find_collection.meta.as_ref().and_then(meta_id) {
                    None => {
                        return Err(AppError::NotFound("Collection meta not found!".to_string()));
                    }
                    Some(id) => id,
                }
            };

            let mut meta: Map<String, Value> = match serde_json::to_value(&meta) {
                Err(_) => {
                    return Err(AppError::BadRequest("Invalid field meta".to_string()));
                }
                Ok(meta) => meta
                    .as_object()
                    .cloned()
                    .ok_or_else(|| AppError::BadRequest("Invalid field meta".to_string()))?,
            };
            meta.insert("collection_id".to_string(), collection_id.into());
            meta_insert(
                tx_ref,
                &self.app_context.schema_name(),
                "alcedo_fields",
                &meta,
            )
            .await?;
        }

        let sql = {
            let mut stmt = TableAlterStatement::new()
                .table((
                    Alias::new(self.app_context.schema_name()),
                    Alias::new(table),
                ))
                .add_col(&field.name.clone(), |mut col| {
                    let needs_special_default = match field.col_type.as_str() {
                        "Date" | "DateTime" => true,
                        _ => false,
                    };

                    match field.col_type.as_str() {
                        "Integer" => {
                            col.integer();

                            if let Some(auto_increment) = field.has_auto_increment {
                                if auto_increment {
                                    col.auto_increment();
                                }
                            }
                        }
                        "Float" => {
                            col.float();

                            if let Some(nprecision) = field.numeric_precision {
                                if let Some(nscale) = field.numeric_scale {
                                    col.decimal_len(nprecision, nscale);
                                }
                            }
                        }
                        "Boolean" => {
                            col.boolean();
                        }
                        "Date" => {
                            col.date();

                            if let Some(default_value) = &field.default_value {
                                match default_value.to_uppercase().as_str() {
                                    "CURRENT_DATE" => {
                                        col.default(Expr::cust("CURRENT_DATE"));
                                    }
                                    _ => {
                                        col.default(default_value.clone());
                                    }
                                }
                            }
                        }
                        "DateTime" => {
                            col.timestamp();

                            if let Some(default_value) = &field.default_value {
                                match default_value.to_uppercase().as_str() {
                                    "CURRENT_TIMESTAMP" => {
                                        col.default(Expr::cust("CURRENT_TIMESTAMP"));
                                    }
                                    "NOW()" => {
                                        col.default(Expr::cust("NOW()"));
                                    }
                                    _ => {
                                        col.default(default_value.clone());
                                    }
                                }
                            }
                        }
                        "JSONB" => {
                            col.json_binary();
                        }
                        _ => {
                            col.string();

                            if let Some(max_length) = field.max_length {
                                col.string_len(max_length);
                            }
                        }
                    };
                    if let Some(is_nullable) = field.is_nullable {
                        if !is_nullable {
                            col.not_null();
                        }
                    } else {
                        col.not_null();
                    }

                    if !needs_special_default {
                        if let Some(default_value) = field.default_value {
                            col.default(default_value);
                        }
                    }

                    col.to_owned()
                })
                .to_owned();
            if let Some(fk) = field.foreign_key {
                let app_name = fk
                    .schema
                    .split_once("010")
                    .unwrap_or((&fk.schema, ""))
                    .0
                    .to_string();
                let to_context = AppContext {
                    app_name,
                    request_source: self.app_context.request_source.clone(),
                    version: self.app_context.version.clone(),
                };
                println!(
                    "Adding fk! {:?} {:?} {:?}",
                    &self.app_context, &to_context, fk
                );
                stmt = stmt
                    .add_fk(
                        &self.app_context,
                        table,
                        &field.name.clone(),
                        &to_context,
                        &fk.table,
                        &fk.column,
                    )
                    .to_owned();
            }
            stmt.to_string(PostgresQueryBuilder)
        };

        exec_tx(tx_ref, &sql).await?;

        if let Some(unique) = field.is_unique {
            if unique {
                let sql = Index::create()
                    .name(format!("uni-{}-{}", table, field.name.clone()))
                    .table((
                        Alias::new(self.app_context.schema_name()),
                        Alias::new(table),
                    ))
                    .col(Alias::new(field.name.clone()))
                    .unique()
                    .to_string(PostgresQueryBuilder);

                exec_tx(tx_ref, &sql).await?;
            }
        }

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };
        self.refresh_schema().await;
        Ok(())
    }

    pub async fn drop_field(
        &self,
        table: &str,
        field: &str,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AppError> {
        let pool = self.pool()?.clone();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref: &mut Transaction<'_, Postgres> = match database_transaction {
            Some(existing) => &mut **existing,
            None => {
                owned_tx = Some(pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };

        {
            let read_guard = self.core.schema.read().await;
            let find_field = read_guard.columns.iter().find(|col| {
                col.schema == self.app_context.schema_name()
                    && col.table == table
                    && col.name == field
            });

            if let Some(field) = &find_field {
                if let Some(meta) = &field.meta {
                    if let Some(id) = meta_id(meta) {
                        meta_delete_by_id(
                            tx_ref,
                            &self.app_context.schema_name(),
                            "alcedo_fields",
                            id,
                        )
                        .await?;
                    }
                }
            }
        }

        let sql = {
            let stmt = TableAlterStatement::new()
                .table((
                    Alias::new(self.app_context.schema_name()),
                    Alias::new(table),
                ))
                .drop_column(Alias::new(field))
                .to_owned();
            stmt.to_string(PostgresQueryBuilder)
        };
        // NOTE: the prototype ran this DDL on the pool even when a
        // transaction was supplied; it now joins the caller's transaction.
        exec_tx(tx_ref, &sql).await?;

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };

        self.refresh_schema().await;
        Ok(())
    }

    pub async fn update_field(
        &self,
        table: &str,
        api_name: &str,
        schema: FieldUpdateObject,
        meta: Option<FieldMetaObject>,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AppError> {
        let pool = self.pool()?.clone();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref: &mut Transaction<'_, Postgres> = match database_transaction {
            Some(existing) => &mut **existing,
            None => {
                owned_tx = Some(pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };
        {
            let read_guard = self.core.schema.read().await;
            let find_field = read_guard.columns.iter().find(|field| {
                field.schema == self.app_context.schema_name()
                    && field.table == table
                    && field.name == api_name.to_string()
            });

            let mut meta: Map<String, Value> = if let Some(meta) = meta {
                match serde_json::to_value(&meta) {
                    Err(_) => {
                        return Err(AppError::BadRequest("Invalid field meta".to_string()));
                    }
                    Ok(meta) => meta
                        .as_object()
                        .cloned()
                        .ok_or_else(|| AppError::BadRequest("Invalid field meta".to_string()))?,
                }
            } else {
                Map::new()
            };

            let saved_id = find_field
                .as_ref()
                .and_then(|f| f.meta.as_ref())
                .and_then(meta_id);

            if let Some(new_api_name) = &schema.name {
                meta.insert("api_name".to_string(), new_api_name.clone().into());
            } else if saved_id.is_none() {
                meta.insert("api_name".to_string(), api_name.to_string().into());
            }

            match saved_id {
                Some(id) => {
                    meta_update_by_id(
                        tx_ref,
                        &self.app_context.schema_name(),
                        "alcedo_fields",
                        id,
                        &meta,
                    )
                    .await?;
                }
                None => {
                    meta_insert(
                        tx_ref,
                        &self.app_context.schema_name(),
                        "alcedo_fields",
                        &meta,
                    )
                    .await?;
                }
            }
        }

        if schema.name.is_some()
            || schema.default_value.is_some()
            || schema.is_nullable.is_some()
            || schema.is_unique.is_some()
            || schema.max_length.is_some()
            || schema.numeric_precision.is_some()
            || schema.numeric_scale.is_some()
        {
            if let Some(name) = schema.name {
                let sql = {
                    let mut stmt = TableAlterStatement::new()
                        .table((
                            Alias::new(self.app_context.schema_name()),
                            Alias::new(table),
                        ))
                        .to_owned();

                    stmt.rename_column(Alias::new(api_name), Alias::new(name));
                    stmt.to_string(PostgresQueryBuilder)
                };
                exec_tx(tx_ref, &sql).await?;
            }

            if let Some(unique) = schema.is_unique {
                let unique_key = format!("{}-uniq", api_name);
                if unique {
                    let sql = Index::create()
                        .name(unique_key)
                        .table((
                            Alias::new(self.app_context.schema_name()),
                            Alias::new(table),
                        ))
                        .col(Alias::new(api_name))
                        .unique()
                        .to_string(PostgresQueryBuilder);
                    exec_tx(tx_ref, &sql).await?;
                } else {
                    let sql = Index::drop()
                        .name(unique_key)
                        .table((
                            Alias::new(self.app_context.schema_name()),
                            Alias::new(table),
                        ))
                        .build(PostgresQueryBuilder);
                    exec_tx(tx_ref, &sql).await?;
                }
            }

            if schema.default_value.is_some()
                || schema.is_nullable.is_some()
                || schema.max_length.is_some()
                || schema.numeric_precision.is_some() && schema.numeric_scale.is_some()
            {
                let sql = {
                    let mut stmt = TableAlterStatement::new()
                        .table((
                            Alias::new(self.app_context.schema_name()),
                            Alias::new(table),
                        ))
                        .to_owned();

                    let mut column_update = ColumnDef::new(Alias::new(api_name));

                    if let Some(default_value) = schema.default_value {
                        column_update.default(default_value);
                    }
                    if let Some(nullable) = schema.is_nullable {
                        if nullable {
                            column_update.null();
                        } else {
                            column_update.not_null();
                        }
                    }

                    if let Some(max_length) = schema.max_length {
                        column_update.string_len(max_length);
                    }

                    if let Some(nprecision) = schema.numeric_precision {
                        if let Some(nscale) = schema.numeric_scale {
                            column_update.decimal_len(nprecision, nscale);
                        }
                    }
                    stmt.modify_column(column_update);
                    stmt.to_string(PostgresQueryBuilder)
                };
                exec_tx(tx_ref, &sql).await?;
            }
        }

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };

        self.refresh_schema().await;
        Ok(())
    }
}

pub async fn get_pk_key(
    schema: &RwLock<CoreDatabaseSchema>,
    collection: &str,
) -> Result<CoreColumn, AppError> {
    let schema = schema.read().await;
    let pk = schema
        .columns
        .iter()
        .find(|col| col.table == collection && col.is_primary_key);

    if let None = pk {
        return Err(AppError::NotFound(format!(
            "No primary key found on table {}",
            collection
        )));
    }

    Ok(pk.unwrap().clone())
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Build a [`CoreState`] for tests from `DATABASE_URL`.
    async fn test_core() -> CoreState {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for alcedo-db tests");
        let pool = crate::db::connect_pool(&url)
            .await
            .expect("Failed to connect test pool");
        CoreState::new(
            Some(pool),
            alcedo_common::config::AppConfig::from_env().expect("Failed to load AppConfig"),
        )
    }

    async fn exec_sql(pool: &Pool, sql: &str) -> Result<(), AppError> {
        sqlx::query(sql).execute(pool).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_create_table() {
        let state = test_core().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
        };
        let table_manager = TableService::new(&state, &context);

        table_manager
            .create_table(
                "automated_create_test",
                |builder| {
                    let mut column = ColumnDef::new(Alias::new("abc"));

                    builder.col(column.string());
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not create test table!");

        exec_sql(state.pool().unwrap(), "drop table automated_create_test")
            .await
            .expect("Error: could not remove test table");
        ()
    }

    #[tokio::test]
    async fn test_drop_table() {
        let state = test_core().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
        };
        let table_manager = TableService::new(&state, &context);

        table_manager
            .create_table(
                "automated_drop_test",
                |builder| {
                    let mut column = ColumnDef::new(Alias::new("abc"));

                    builder.col(column.string());
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not create test table!");

        table_manager
            .drop_table("automated_drop_test", &mut None)
            .await
            .expect("Error: could not drop test table");
        ()
    }

    #[tokio::test]
    async fn test_add_field_table() {
        let state = test_core().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
        };
        let table_manager = TableService::new(&state, &context);

        table_manager
            .create_table(
                "automated_add_field_test",
                |builder| {
                    let mut column = ColumnDef::new(Alias::new("abc"));

                    builder.col(column.string());
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not create test table!");

        table_manager
            .add_field(
                "automated_add_field_test",
                FieldCreationObject {
                    name: "test_field_int".to_string(),
                    col_type: "int".to_string(),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add int field");
        table_manager
            .add_field(
                "automated_add_field_test",
                FieldCreationObject {
                    name: "test_field_text".to_string(),
                    col_type: "text".to_string(),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add text field");
        table_manager
            .add_field(
                "automated_add_field_test",
                FieldCreationObject {
                    name: "test_field_bool".to_string(),
                    col_type: "bool".to_string(),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add bool field");
        table_manager
            .add_field(
                "automated_add_field_test",
                FieldCreationObject {
                    name: "test_field_float".to_string(),
                    col_type: "float".to_string(),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add float field");
        table_manager
            .add_field(
                "automated_add_field_test",
                FieldCreationObject {
                    name: "test_field_random".to_string(),
                    col_type: "random".to_string(),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add random field");
        table_manager
            .add_field(
                "automated_add_field_test",
                FieldCreationObject {
                    name: "test_field_date".to_string(),
                    col_type: "Date".to_string(),
                    default_value: Some("CURRENT_DATE".to_string()),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add date field");
        table_manager
            .add_field(
                "automated_add_field_test",
                FieldCreationObject {
                    name: "test_field_datetime".to_string(),
                    col_type: "DateTime".to_string(),
                    default_value: Some("CURRENT_TIMESTAMP".to_string()),
                    is_nullable: Some(false),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add datetime field");
        table_manager
            .drop_table("automated_add_field_test", &mut None)
            .await
            .expect("Error: could not drop test table");
        ()
    }
    #[tokio::test]
    async fn test_remove_field_table() {
        let state = test_core().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
        };
        let table_manager = TableService::new(&state, &context);

        table_manager
            .create_table(
                "automated_remove_field_test",
                |builder| {
                    let mut column = ColumnDef::new(Alias::new("abc"));

                    builder.col(column.string());
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not create test table!");

        table_manager
            .add_field(
                "automated_remove_field_test",
                FieldCreationObject {
                    name: "test_field_float".to_string(),
                    col_type: "float".to_string(),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add float field");
        table_manager
            .drop_field("automated_remove_field_test", "test_field_float", &mut None)
            .await
            .expect("Error: could not drop float field");
        table_manager
            .drop_table("automated_remove_field_test", &mut None)
            .await
            .expect("Error: could not drop test table");
        ()
    }
    #[tokio::test]
    async fn test_rename_field_table() {
        let state = test_core().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
        };
        let table_manager = TableService::new(&state, &context);

        table_manager
            .create_table(
                "automated_rename_field_test",
                |builder| {
                    let mut column = ColumnDef::new(Alias::new("abc"));

                    builder.col(column.string());
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not create test table!");

        table_manager
            .add_field(
                "automated_rename_field_test",
                FieldCreationObject {
                    name: "test_field_float".to_string(),
                    col_type: "float".to_string(),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add float field");
        table_manager
            .update_field(
                "automated_rename_field_test",
                "test_field_float",
                FieldUpdateObject {
                    name: Some("test_field_float_2".to_string()),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .expect("Error: could not add float field");
        table_manager
            .drop_table("automated_rename_field_test", &mut None)
            .await
            .expect("Error: could not drop test table");
        ()
    }
}
