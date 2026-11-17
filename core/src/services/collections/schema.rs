use std::collections::HashMap;

use sea_query::{
    Alias, ColumnDef, ForeignKey, ForeignKeyAction, PostgresQueryBuilder, Table,
    TableAlterStatement, TableCreateStatement,
};

use super::ddl::{
    build_add_unique_sql, build_drop_columns_sqls, column_def, drop_table_sql, fk_constraint,
    spec_from_field_creation,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{Postgres, Transaction};
use tokio::sync::RwLock;
use utoipa::ToSchema;

use crate::services::items::query::{Comparison, FieldValue};
use crate::services::postgres::pool::execute_query_transaction;
use crate::{
    AppState,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{
            query::{FieldFilter, Filter, LogicOp, Query},
            service::ItemsService,
        },
        postgres::{
            self,
            inspector::{Column, DatabaseSchema, TableFields, TableMeta},
        },
    },
};

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

/// Drops a schema and every object inside it. Callers are responsible for
/// refreshing the schema cache afterwards.
pub async fn drop_schema(state: &AppState, schema_name: &str) -> Result<(), AlcedoError> {
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", schema_name))
        .execute(&*state.database_pool)
        .await?;
    Ok(())
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
    pub foreign_key: Option<postgres::inspector::ForeignKey>,
}

#[derive(ToSchema, Serialize, Default, Deserialize, Debug, Clone)]
pub struct FieldSavedMetaObject {
    pub api_name: String,
    pub display_name: String,
    pub collection_id: Option<i32>,
    pub id: i32,
    pub options: Option<serde_json::Value>,
    #[serde(default)]
    pub ordinal_position: Option<i32>,
}

#[derive(ToSchema, Serialize, Default, Deserialize, Debug, Clone)]
pub struct FieldMetaObject {
    pub api_name: String,
    pub display_name: String,
    pub collection_id: Option<i64>,
    pub options: Option<serde_json::Value>,
}

/// Serializes every publisher of the shared schema cache, so concurrent
/// refreshes cannot interleave and an older snapshot cannot overwrite a
/// newer one.
pub(crate) fn refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static REFRESH_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
        std::sync::OnceLock::new();
    REFRESH_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub struct SchemaService<'a> {
    app_state: &'a AppState,
    app_context: &'a AppContext,
}

impl SchemaService<'_> {
    pub fn new<'a>(app_state: &'a AppState, context: &'a AppContext) -> SchemaService<'a> {
        return SchemaService {
            app_state,
            app_context: context,
        };
    }

    pub async fn refresh_schema(&self) -> Result<(), AlcedoError> {
        // Single-flight: a burst of concurrent DDL commits (e.g. many
        // collection creates at once) would otherwise each run a full
        // introspection + meta refresh. Serialize them; each caller still
        // introspects after its own commit, so no refresh is skipped stale.
        let _refresh_guard = refresh_lock().lock().await;

        // Build the complete new schema (introspection + meta) without holding
        // any lock, then swap the whole struct under a single write guard so
        // readers never observe a torn mix of new tables with stale meta.
        let mut new_schema = DatabaseSchema::refresh(self.app_state).await;
        let save_cache = match self.app_context.request_source {
            RequestSource::FirstMigration => false,
            _ => {
                // A failed meta read must not publish a schema without meta.
                SchemaService::refresh_all_meta_into(self.app_state, &mut new_schema).await?;
                true
            }
        };
        *self.app_state.database_schema.write().await = new_schema;
        if save_cache {
            self.app_state.save_schema_cache().await;
        }
        Ok(())
    }

    /// Reads the collection/field meta rows for `ctx` and applies them onto
    /// `schema` in memory. Holds no locks — callers swap the finished schema
    /// into the cache atomically under a single write guard.
    pub async fn refresh_meta_into(
        state: &AppState,
        ctx: &AppContext,
        schema: &mut DatabaseSchema,
    ) -> Result<(), AlcedoError> {
        // Read the meta tables with raw SQL: going through ItemsService would
        // resolve the tables against the live schema cache, which must not be
        // required here — this function builds the schema that replaces it.
        async fn read_meta_table(
            state: &AppState,
            ctx: &AppContext,
            table: &str,
        ) -> Result<Vec<Map<String, Value>>, AlcedoError> {
            // Propagate the error: the caller swaps the whole schema into the
            // live cache, so silently defaulting here would wipe all
            // collection/field meta from the cache without any trace.
            let rows = postgres::pool::execute_query(
                state,
                format!("SELECT * FROM {}", super::ddl::qtable(ctx, table)),
            )
            .await?;
            Ok(rows
                .iter()
                .filter_map(|row| match postgres::pool::pgrow_to_json(row) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        tracing::error!("Failed to decode meta row from {}: {}", table, e);
                        None
                    }
                })
                .collect())
        }

        let meta_collections = read_meta_table(state, ctx, "alcedocore_collections").await?;
        let meta_fields = read_meta_table(state, ctx, "alcedocore_fields").await?;

        for table in schema.tables.clone() {
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
                let meta: TableMeta =
                    serde_json::from_value(serde_json::Value::Object(meta.clone())).unwrap();

                {
                    let find_table = schema
                        .tables
                        .iter_mut()
                        .find(|t| t.schema == table.schema && meta.table == t.name);

                    if let Some(find_table) = find_table {
                        find_table.meta = Some(meta.clone());
                    }
                }

                let collection_id = meta.id.unwrap();

                let fields: Vec<_> = meta_fields
                    .iter()
                    .filter(|field| {
                        field.get("collection_id").and_then(|v| v.as_i64()) == Some(collection_id)
                    })
                    .collect();

                for field in &fields {
                    let meta: FieldSavedMetaObject =
                        serde_json::from_value(serde_json::Value::Object((*field).clone()))
                            .unwrap();

                    let find = schema.columns.iter_mut().find(|col| {
                        col.name == meta.api_name
                            && col.table == table.name
                            && col.schema == table.schema
                    });

                    if let Some(col) = find {
                        col.meta = Some(meta);
                    }
                }

                let field_defs: Vec<super::FieldDefinition> =
                    fields.iter().map(|f| super::field_from_row(f)).collect();
                schema
                    .fields
                    .retain(|tf| !(tf.schema == table.schema && tf.table == table.name));
                schema.fields.push(TableFields {
                    schema: table.schema.clone(),
                    table: table.name.clone(),
                    fields: field_defs,
                });
            }
        }
        Ok(())
    }

    /// Applies meta for every app version onto `schema` without holding locks.
    pub async fn refresh_all_meta_into(
        state: &AppState,
        schema: &mut DatabaseSchema,
    ) -> Result<(), AlcedoError> {
        for version in schema.app_versions.clone() {
            let ctx = AppContext {
                app_name: version.app_name,
                version: version.version_name,
                request_source: RequestSource::Inspector,
                identity: None,
                request_id: None,
            };
            SchemaService::refresh_meta_into(state, &ctx, schema).await?;
        }
        Ok(())
    }

    pub async fn refresh_all_meta(state: &AppState) -> Result<(), AlcedoError> {
        let _refresh_guard = refresh_lock().lock().await;
        let mut new_schema = state.database_schema.read().await.clone();
        SchemaService::refresh_all_meta_into(state, &mut new_schema).await?;
        *state.database_schema.write().await = new_schema;
        state.save_schema_cache().await;
        Ok(())
    }

    /// Refreshes the in-memory schema and repopulates collection meta, swapping
    /// the whole struct atomically so readers never see a torn schema. Meta
    /// reads use raw SQL (see `refresh_meta_into`), so this works even on a
    /// fresh `AppState` whose live cache is still empty.
    pub async fn refresh_schema_and_meta(state: &AppState) -> Result<(), AlcedoError> {
        let _refresh_guard = refresh_lock().lock().await;
        let mut new_schema = DatabaseSchema::refresh(state).await;
        SchemaService::refresh_all_meta_into(state, &mut new_schema).await?;
        *state.database_schema.write().await = new_schema;
        state.save_schema_cache().await;
        Ok(())
    }

    pub async fn create_table<F>(
        &self,
        name: &str,
        builder: F,
        meta: Option<TableMeta>,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AlcedoError>
    where
        F: FnOnce(&mut TableCreateStatement),
    {
        let owns_tx = database_transaction.is_none();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref = match database_transaction {
            Some(existing) => existing,
            None => {
                owned_tx = Some(self.app_state.database_pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };
        if let Some(meta) = meta {
            let collection = "alcedocore_collections".to_string();
            let mut collections_service =
                ItemsService::new(self.app_state, self.app_context, &collection);

            let meta: Map<String, Value> = match serde_json::to_value(&meta) {
                Err(_) => {
                    return Err(AlcedoError::InvalidInput(
                        "Invalid table meta".to_string(),
                        1,
                    ));
                }
                Ok(meta) => meta.as_object().cloned().ok_or_else(|| {
                    AlcedoError::InvalidInput("Invalid table meta".to_string(), 1)
                })?,
            };
            collections_service
                .create_many(vec![meta], &mut Some(tx_ref))
                .await?;
        }

        let sql = {
            let mut stmt = Table::create();

            stmt.table((Alias::new(self.app_context.schema_name()), Alias::new(name)));
            builder(&mut stmt);
            stmt.to_string(PostgresQueryBuilder)
        };
        execute_query_transaction(&self.app_state, tx_ref, &sql).await?;

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };
        if owns_tx {
            self.refresh_schema().await?;
        }

        Ok(())
    }

    pub async fn drop_table(
        &self,
        name: &str,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AlcedoError> {
        let owns_tx = database_transaction.is_none();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref = match database_transaction {
            Some(existing) => existing,
            None => {
                owned_tx = Some(self.app_state.database_pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };
        // Look up the table meta under the read guard, then drop the guard
        // before the delete awaits: those re-enter `read()` on the same
        // RwLock, which deadlocks while the guard is held and a writer is
        // queued.
        let meta_id = {
            let read_guard = self.app_state.database_schema.read().await;
            let find_table = read_guard
                .tables
                .iter()
                .find(|table| table.schema == self.app_context.schema_name() && table.name == name);

            if let None = find_table {
                return Err(AlcedoError::NotFound(
                    format!("The table {} could not be found.", name),
                    1,
                ));
            }

            find_table.unwrap().meta.as_ref().and_then(|meta| meta.id)
        };

        if let Some(meta_id) = meta_id {
            let collection = "alcedocore_fields".to_string();
            let mut column_meta_service =
                ItemsService::new(&self.app_state, &self.app_context, &collection);

            let mut query = Query {
                ..Default::default()
            };

            let mut hmap = FieldFilter {
                fields: HashMap::new(),
            };

            hmap.fields.insert(
                "collection_id".to_string(),
                FieldValue::Comparison(Comparison {
                    _eq: Some(meta_id.into()),
                    ..Default::default()
                }),
            );
            query.filter = LogicOp {
                _and: Some(vec![Filter::Field(hmap)]),
                _or: None,
            };
            column_meta_service
                .delete_items_by_query(query, &mut Some(tx_ref))
                .await?;

            let collection = "alcedocore_collections".to_string();
            let mut table_meta_service =
                ItemsService::new(&self.app_state, &self.app_context, &collection);

            table_meta_service
                .delete_items_by_pks(vec![meta_id.into()], Some(tx_ref))
                .await?;
        }

        let sql = drop_table_sql(&self.app_context, name, false);
        execute_query_transaction(&self.app_state, tx_ref, &sql).await?;

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };
        if owns_tx {
            self.refresh_schema().await?;
        }
        Ok(())
    }

    pub async fn add_field(
        &self,
        table: &str,
        field: FieldCreationObject,
        meta: Option<FieldMetaObject>,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AlcedoError> {
        let owns_tx = database_transaction.is_none();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref = match database_transaction {
            Some(existing) => existing,
            None => {
                owned_tx = Some(self.app_state.database_pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };
        if let Some(meta) = meta {
            let collection_id = {
                let read_guard = self.app_state.database_schema.read().await;
                let find_collection = read_guard.tables.iter().find(|collection| {
                    collection.schema == self.app_context.schema_name() && collection.name == table
                });
                if let None = find_collection {
                    return Err(AlcedoError::NotFound(
                        "Collection not found!".to_string(),
                        1,
                    ));
                };
                let find_collection = find_collection.unwrap();
                if let None = find_collection.meta {
                    return Err(AlcedoError::NotFound(
                        "Collection meta not found!".to_string(),
                        1,
                    ));
                } else {
                    find_collection.meta.clone().unwrap().id.unwrap()
                }
            };

            let collection = "alcedocore_fields".to_string();
            let mut collections_service =
                ItemsService::new(self.app_state, self.app_context, &collection);

            let mut meta: Map<String, Value> = match serde_json::to_value(&meta) {
                Err(_) => {
                    return Err(AlcedoError::InvalidInput(
                        "Invalid field meta".to_string(),
                        1,
                    ));
                }
                Ok(meta) => meta.as_object().cloned().ok_or_else(|| {
                    AlcedoError::InvalidInput("Invalid field meta".to_string(), 1)
                })?,
            };
            meta.insert("collection_id".to_string(), collection_id.into());
            collections_service
                .create_many(vec![meta], &mut Some(tx_ref))
                .await?;
        }

        let sql = TableAlterStatement::new()
            .table((
                Alias::new(self.app_context.schema_name()),
                Alias::new(table),
            ))
            .add_column(column_def(spec_from_field_creation(&field)?))
            .to_string(PostgresQueryBuilder);
        execute_query_transaction(&self.app_state, tx_ref, &sql).await?;

        if let Some(fk) = field.foreign_key.clone() {
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
                identity: None,
                request_id: self.app_context.request_id.clone(),
            };
            let fk_sql = fk_constraint(
                &format!("{}_{}_{}_{}", table, field.name, fk.table, fk.column),
                &self.app_context.schema_name(),
                table,
                &field.name,
                &to_context.schema_name(),
                &fk.table,
                &fk.column,
                ForeignKeyAction::NoAction,
                ForeignKeyAction::NoAction,
            );
            execute_query_transaction(&self.app_state, tx_ref, &fk_sql).await?;
        }

        if field.is_unique == Some(true) {
            let unique_sql = build_add_unique_sql(&self.app_context, table, &field.name);
            execute_query_transaction(&self.app_state, tx_ref, &unique_sql).await?;
        }

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };
        if owns_tx {
            self.refresh_schema().await?;
        }
        Ok(())
    }

    pub async fn drop_field(
        &self,
        table: &str,
        field: &str,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AlcedoError> {
        let owns_tx = database_transaction.is_none();
        let mut owned_tx: Option<Transaction<Postgres>> = None;
        let tx_ref = match database_transaction {
            Some(existing) => existing,
            None => {
                owned_tx = Some(self.app_state.database_pool.begin().await?);
                owned_tx.as_mut().unwrap()
            }
        };

        // Look up the field meta under the read guard, then drop the guard
        // before the delete await: `delete_items_by_pks` re-enters `read()` on
        // the same RwLock, which deadlocks while the guard is held and a
        // writer is queued.
        let field_meta_id = {
            let read_guard = self.app_state.database_schema.read().await;
            read_guard
                .columns
                .iter()
                .find(|col| {
                    col.schema == self.app_context.schema_name()
                        && col.table == table
                        && col.name == field
                })
                .and_then(|col| col.meta.as_ref())
                .map(|meta| meta.id)
        };

        if let Some(meta_id) = field_meta_id {
            let collection = "alcedocore_fields".to_string();
            let mut collections_service =
                ItemsService::new(self.app_state, self.app_context, &collection);
            collections_service
                .delete_items_by_pks(vec![meta_id.into()], Some(tx_ref))
                .await?;
        }

        for sql in build_drop_columns_sqls(&self.app_context, table, &[field]) {
            execute_query_transaction(&self.app_state, tx_ref, &sql).await?;
        }

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };
        if owns_tx {
            self.refresh_schema().await?;
        }
        Ok(())
    }
}

pub async fn get_pk_key<'a>(
    schema: &RwLock<DatabaseSchema>,
    schema_name: &str,
    collection: &'a str,
) -> Result<Column, AlcedoError> {
    let schema = schema.read().await;
    let pk = schema
        .columns
        .iter()
        .find(|col| col.table == collection && col.schema == schema_name && col.is_primary_key);

    if let None = pk {
        return Err(AlcedoError::SystemError(
            format!("No primary key found on table {}", collection),
            1,
        ));
    }

    Ok(pk.unwrap().clone())
}

#[cfg(test)]
mod tests {

    use crate::services::postgres::pool::execute_query;
    use crate::utils;

    use super::*;

    #[tokio::test]
    async fn test_create_table() {
        let state = utils::test_utils::get_app_state().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::SystemTest,
            identity: None,
            request_id: None,
        };
        let table_manager = SchemaService::new(&state, &context);

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

        execute_query(
            &state,
            format!("drop table {}.automated_create_test", context.schema_name()),
        )
        .await
        .expect("Error: could not remove test table");
        ()
    }

    #[tokio::test]
    async fn test_drop_table() {
        let state = utils::test_utils::get_app_state().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::SystemTest,
            identity: None,
            request_id: None,
        };
        let table_manager = SchemaService::new(&state, &context);

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
        let state = utils::test_utils::get_app_state().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::SystemTest,
            identity: None,
            request_id: None,
        };
        let table_manager = SchemaService::new(&state, &context);

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
        let state = utils::test_utils::get_app_state().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::SystemTest,
            identity: None,
            request_id: None,
        };
        let table_manager = SchemaService::new(&state, &context);

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
}
