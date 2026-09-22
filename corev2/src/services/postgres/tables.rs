use std::collections::HashMap;

use futures::FutureExt;
use sea_query::{
    Alias, ColumnDef, Expr, ForeignKey, Index, PostgresQueryBuilder, Table, TableAlterStatement,
    TableCreateStatement, TableForeignKey,
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
            pool::execute_query,
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
    app_state: &'a AppState,
    app_context: &'a AppContext,
}

impl TableService<'_> {
    pub fn new<'a>(app_state: &'a AppState, context: &'a AppContext) -> TableService<'a> {
        return TableService {
            app_state,
            app_context: context,
        };
    }

    pub async fn refresh_schema(&self) {
        let mut write_schema_lock = self.app_state.database_schema.write().await;
        let refresh_schema = write_schema_lock.refresh(&self.app_state).await;
        write_schema_lock.columns = refresh_schema.columns;
        write_schema_lock.tables = refresh_schema.tables;
        write_schema_lock.app_versions = refresh_schema.app_versions;
        drop(write_schema_lock);

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
            .unwrap();

        let meta_fields = meta_fields_service
            .read_items_by_query(Query {
                ..Default::default()
            })
            .await
            .unwrap();

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

        self.refresh_schema().await;

        Ok(())
    }

    pub async fn drop_table(
        &self,
        name: &str,
        database_transaction: &mut Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<(), AlcedoError> {
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

        let sql = {
            let stmt = Table::drop()
                .table((Alias::new(self.app_context.schema_name()), Alias::new(name)))
                .if_exists()
                .to_owned();
            stmt.to_string(PostgresQueryBuilder)
        };

        execute_query_transaction(&self.app_state, tx_ref, &sql).await?;

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
    ) -> Result<(), AlcedoError> {
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

        execute_query_transaction(&self.app_state, tx_ref, &sql).await?;

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

                execute_query_transaction(&self.app_state, tx_ref, &sql).await?;
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
    ) -> Result<(), AlcedoError> {
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
        execute_query(&self.app_state, sql).await?;

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
    ) -> Result<(), AlcedoError> {
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
            let find_field = read_guard.columns.iter().find(|field| {
                field.schema == self.app_context.schema_name()
                    && field.table == table
                    && field.name == api_name.to_string()
            });
            let collection = "alcedo_fields".to_string();
            let fields_service = ItemsService::new(self.app_state, self.app_context, &collection);

            let mut meta: Map<String, Value> = if let Some(meta) = meta {
                match serde_json::to_value(&meta) {
                    Err(_) => {
                        return Err(AlcedoError::InvalidInput(
                            "Invalid field meta".to_string(),
                            1,
                        ));
                    }
                    Ok(meta) => meta.as_object().cloned().ok_or_else(|| {
                        AlcedoError::InvalidInput("Invalid field meta".to_string(), 1)
                    })?,
                }
            } else {
                Map::new()
            };

            if let Some(find_field) = find_field {
                if let Some(saved_meta) = &find_field.meta {
                    if let Some(new_api_name) = &schema.name {
                        meta.insert("api_name".to_string(), new_api_name.clone().into());
                    }

                    let mut id_filter = FieldFilter {
                        fields: HashMap::new(),
                    };
                    id_filter.fields.insert(
                        "id".to_string(),
                        FieldValue::Comparison(Comparison {
                            _eq: Some(saved_meta.id.into()),
                            ..Default::default()
                        }),
                    );

                    fields_service
                        .update_items_by_query(
                            &mut Query {
                                filter: LogicOp {
                                    _and: Some(vec![Filter::Field(id_filter)]),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            meta,
                            &mut Some(tx_ref),
                        )
                        .await?;
                } else {
                    if let Some(new_api_name) = &schema.name {
                        meta.insert("api_name".to_string(), new_api_name.clone().into());
                    } else {
                        meta.insert("api_name".to_string(), api_name.to_string().into());
                    }
                    fields_service
                        .create_many(vec![meta], &mut Some(tx_ref))
                        .await?;
                }
            } else {
                if let Some(new_api_name) = &schema.name {
                    meta.insert("api_name".to_string(), new_api_name.clone().into());
                } else {
                    meta.insert("api_name".to_string(), api_name.to_string().into());
                }
                fields_service
                    .create_many(vec![meta], &mut Some(tx_ref))
                    .await?;
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
                execute_query_transaction(&self.app_state, tx_ref, &sql).await?;
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
                    execute_query_transaction(&self.app_state, tx_ref, &sql).await?;
                } else {
                    execute_query_transaction(
                        &self.app_state,
                        tx_ref,
                        &Index::drop()
                            .name(unique_key)
                            .table((
                                Alias::new(self.app_context.schema_name()),
                                Alias::new(table),
                            ))
                            .build(PostgresQueryBuilder),
                    )
                    .await?;
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
                execute_query_transaction(&self.app_state, tx_ref, &sql).await?;
            }
        }

        if let Some(tx) = owned_tx {
            tx.commit().await.unwrap();
        };

        self.refresh_schema().await;
        Ok(())
    }
}

pub async fn get_pk_key<'a>(
    schema: &RwLock<DatabaseSchema>,
    collection: &'a str,
) -> Result<Column, AlcedoError> {
    let schema = schema.read().await;
    let pk = schema
        .columns
        .iter()
        .find(|col| col.table == collection && col.is_primary_key);

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
        let state = utils::test_utils::get_app_state().await;
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
        let state = utils::test_utils::get_app_state().await;
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
        let state = utils::test_utils::get_app_state().await;
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
