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
            inspector::{Column, DatabaseSchema, TableMeta},
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

    pub async fn refresh_schema(&self) {
        self.app_state.refresh_schema().await;

        if self.app_context.app_name == "alcedo" {
            return;
        }

        match self.app_context.request_source {
            RequestSource::FirstMigration => (),
            _ => self.refresh_meta().await,
        };
    }

    pub async fn refresh_meta(&self) {
        let collection = "alcedo_collections".to_string();
        let meta_collections_service =
            ItemsService::new(self.app_state, self.app_context, &collection);
        let collection = "alcedo_fields".to_string();
        let meta_fields_service = ItemsService::new(self.app_state, self.app_context, &collection);

        let meta_collections = meta_collections_service
            .read_items_by_query(Query {
                ..Default::default()
            })
            .await
            .unwrap_or_default();

        let meta_fields = meta_fields_service
            .read_items_by_query(Query {
                ..Default::default()
            })
            .await
            .unwrap_or_default();

        let mut write_schema_lock = self.app_state.database_schema.write().await;

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
                let meta: TableMeta =
                    serde_json::from_value(serde_json::Value::Object(meta.clone())).unwrap();

                {
                    let find_table = write_schema_lock
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

                for field in fields {
                    let meta: FieldSavedMetaObject =
                        serde_json::from_value(serde_json::Value::Object(field.clone())).unwrap();

                    let find = write_schema_lock.columns.iter_mut().find(|col| {
                        col.name == meta.api_name
                            && col.table == table.name
                            && col.schema == table.schema
                    });

                    if let Some(col) = find {
                        col.meta = Some(meta);
                    }
                }
            }
        }
    }

    pub async fn refresh_all_meta(state: &AppState) {
        let app_versions = state.database_schema.read().await.app_versions.clone();
        for version in app_versions {
            let ctx = AppContext {
                app_name: version.app_name,
                version: version.version_name,
                request_source: RequestSource::Inspector,
            };
            SchemaService::new(state, &ctx).refresh_meta().await;
        }
    }

    /// Refreshes the in-memory schema and repopulates collection meta.
    pub async fn refresh_schema_and_meta(state: &AppState) {
        state.refresh_schema().await;
        SchemaService::refresh_all_meta(state).await;
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
            let collection = "alcedo_collections".to_string();
            let collections_service =
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
            self.refresh_schema().await;
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

        let find_table = find_table.unwrap();

        if let Some(meta) = &find_table.meta {
            if let Some(meta_id) = meta.id {
                let collection = "alcedo_fields".to_string();
                let column_meta_service =
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

                let collection = "alcedo_collections".to_string();
                let table_meta_service =
                    ItemsService::new(&self.app_state, &self.app_context, &collection);

                table_meta_service
                    .delete_items_by_pks(vec![meta_id.into()], Some(tx_ref))
                    .await?;
            }
        }
        drop(read_guard);

        let sql = drop_table_sql(&self.app_context, name, false);
        execute_query_transaction(&self.app_state, tx_ref, &sql).await?;

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };
        if owns_tx {
            self.refresh_schema().await;
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

            let collection = "alcedo_fields".to_string();
            let collections_service =
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
            self.refresh_schema().await;
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

        {
            let read_guard = self.app_state.database_schema.read().await;
            let find_field = read_guard.columns.iter().find(|col| {
                col.schema == self.app_context.schema_name()
                    && col.table == table
                    && col.name == field
            });

            if let Some(field) = &find_field {
                if let Some(meta) = &field.meta {
                    let collection = "alcedo_fields".to_string();
                    let collections_service =
                        ItemsService::new(self.app_state, self.app_context, &collection);
                    collections_service
                        .delete_items_by_pks(vec![meta.id.into()], Some(tx_ref))
                        .await?;
                }
            }
        }

        for sql in build_drop_columns_sqls(&self.app_context, table, &[field]) {
            execute_query_transaction(&self.app_state, tx_ref, &sql).await?;
        }

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };
        if owns_tx {
            self.refresh_schema().await;
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
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
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

        execute_query(&state, "drop table automated_create_test".to_string())
            .await
            .expect("Error: could not remove test table");
        ()
    }

    #[tokio::test]
    async fn test_drop_table() {
        let state = utils::test_utils::get_app_state().await;
        let context = AppContext {
            app_name: "testing".to_string(),
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
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
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
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
            version: "".to_string(),
            request_source: RequestSource::SystemTest,
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
