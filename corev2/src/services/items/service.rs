use std::collections::HashMap;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::join_all;
use sea_query::{Alias, Expr, PostgresQueryBuilder, SimpleExpr};
use serde_json::{Map, Value};
use sqlx::{Postgres, Transaction};

use crate::services::context::AppContext;
use crate::services::hooks::HookContext;
use crate::services::hooks::types::items_create::{ItemsAfterCreate, ItemsBeforeCreate};
use crate::services::hooks::types::items_delete::{ItemsAfterDelete, ItemsBeforeDelete};
use crate::services::hooks::types::items_update::{ItemsAfterUpdate, ItemsBeforeUpdate};
use crate::services::items::query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query};
use crate::services::postgres::jsonvalue_simpleexpr::parse_value;
use crate::services::postgres::pool::{
    execute_query_transaction, pgrow_to_json, process_query_error_response,
};
use crate::services::postgres::tables::get_pk_key;
use crate::{AppState, services::errors::AlcedoError};
pub struct ItemsService<'a> {
    app_state: &'a AppState,
    collection: &'a String,
    app_context: &'a AppContext,
}

impl ItemsService<'_> {
    pub fn new<'a>(
        app_state: &'a AppState,
        context: &'a AppContext,
        collection: &'a String,
    ) -> ItemsService<'a> {
        return ItemsService {
            app_state,
            collection,
            app_context: context,
        };
    }

    pub async fn read_items_by_query(
        &self,
        mut query: Query,
    ) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        Ok(query
            .execute_query(self.app_context, self.app_state, self.collection)
            .await?)
    }
    pub async fn update_items_by_query<'a>(
        &self,
        query: &mut Query,
        payload: Map<String, Value>,
        database_transaction: &'a mut Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<Vec<String>, AlcedoError> {
        match database_transaction {
            Some(existing_tx) => self.update_items_with_tx(existing_tx, query, payload).await,
            None => {
                let mut tx = self.app_state.database_pool.begin().await?;
                let result = self.update_items_with_tx(&mut tx, query, payload).await?;
                tx.commit().await?;
                Ok(result)
            }
        }
    }
    async fn update_items_with_tx<'a>(
        &self,
        tx: &'a mut Transaction<'_, Postgres>,
        query: &mut Query,
        payload: Map<String, Value>,
    ) -> Result<Vec<String>, AlcedoError> {
        let pk_name = get_pk_key(&self.app_state.database_schema, &self.collection)
            .await?
            .name;

        query.fields = vec![pk_name.clone()];

        let get_pks = query
            .execute_query(self.app_context, self.app_state, self.collection)
            .await?;

        let mut before = ItemsBeforeUpdate {
            keys: get_pks
                .clone()
                .iter()
                .map(|item| item.get(&pk_name).unwrap().clone().to_string())
                .collect(),
            payload,
            collection: self.collection.clone(),
        };

        {
            let hook_context = HookContext {
                context: self.app_context.clone(),
                state: self.app_state.clone(),
                tx,
            };

            self.app_state
                .event_bus
                .trigger(
                    &format!("before.items.update.{}", self.collection),
                    &mut before,
                    hook_context,
                )
                .await;
        }

        let queries: Vec<Result<String, AlcedoError>> = join_all(get_pks.iter().map(|item| {
            self.generate_update_item_query(&before.payload, item.get(&pk_name).unwrap().clone())
        }))
        .await;

        let errors: Vec<String> = queries
            .iter()
            .filter_map(|query| query.as_ref().err())
            .map(|err_ref| err_ref.to_string())
            .collect();

        if !errors.is_empty() {
            return Err(AlcedoError::InvalidInput(errors.join(","), 1));
        }

        let mut update_items: Vec<String> = vec![];

        for query in queries {
            let query_str = query.unwrap();
            let result = execute_query_transaction(&self.app_state, tx, &query_str).await?;
            let pk_data = pgrow_to_json(result.get(0).unwrap()).unwrap();
            let pk = pk_data.get(&pk_name).unwrap().to_string();
            update_items.push(pk);
        }

        let mut after = ItemsAfterUpdate {
            keys: update_items.clone(),
            payload: before.payload.clone(),
            collection: self.collection.clone(),
        };

        let hook_context = HookContext {
            context: self.app_context.clone(),
            state: self.app_state.clone(),
            tx,
        };

        self.app_state
            .event_bus
            .trigger(
                &format!("after.items.update.{}", self.collection),
                &mut after,
                hook_context,
            )
            .await;

        Ok(update_items)
    }

    pub async fn create_many<'a>(
        &self,
        items: Vec<Map<String, Value>>,
        database_transaction: &'a mut Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<Vec<String>, AlcedoError> {
        match database_transaction {
            Some(existing_tx) => self.create_items_with_tx(existing_tx, items).await,
            None => {
                let mut tx = self.app_state.database_pool.begin().await?;
                let result = self.create_items_with_tx(&mut tx, items).await?;
                tx.commit().await?;
                Ok(result)
            }
        }
    }

    async fn create_items_with_tx<'a>(
        &self,
        tx: &'a mut Transaction<'_, Postgres>,
        items: Vec<Map<String, Value>>,
    ) -> Result<Vec<String>, AlcedoError> {
        let mut before = ItemsBeforeCreate {
            items,
            collection: self.collection.clone(),
        };

        let hook_context = HookContext {
            context: self.app_context.clone(),
            state: self.app_state.clone(),
            tx,
        };

        self.app_state
            .event_bus
            .trigger(
                &format!("before.items.create.{}", self.collection),
                &mut before,
                hook_context,
            )
            .await;

        let queries: Vec<Result<String, AlcedoError>> = join_all(
            before
                .items
                .clone()
                .iter()
                .map(|item| self.generate_insert_item_query(item.clone())),
        )
        .await;

        let errors: Vec<String> = queries
            .iter()
            .filter_map(|query| query.as_ref().err())
            .map(|err_ref| err_ref.to_string())
            .collect();

        if errors.len() > 0 {
            return Err(AlcedoError::InvalidInput(errors.join(","), 1));
        }

        let pk_name = get_pk_key(&self.app_state.database_schema, &self.collection)
            .await?
            .name;
        let pk_name_cloned = pk_name.clone();
        let mut created_items = vec![];
        for query in queries {
            let query = query.unwrap();
            let result = execute_query_transaction(&self.app_state, tx, &query).await?;
            let pk = pgrow_to_json(result.get(0).unwrap()).unwrap();
            let pk = pk.get(&pk_name).unwrap().to_string();

            created_items.push(pk);
        }

        before
            .items
            .iter_mut()
            .enumerate()
            .for_each(|(index, item)| {
                let pk = created_items[index].to_string();
                item.insert(pk_name_cloned.clone(), pk.into());
            });

        let mut after = ItemsAfterCreate {
            items: before.items,
            collection: self.collection.clone(),
        };

        let hook_context = HookContext {
            context: self.app_context.clone(),
            state: self.app_state.clone(),
            tx,
        };

        self.app_state
            .event_bus
            .trigger(
                &format!("after.items.create.{}", self.collection),
                &mut after,
                hook_context,
            )
            .await;

        Ok(created_items)
    }

    pub async fn delete_items_by_pks<'a>(
        &self,
        pks: Vec<Value>,
        mut database_transaction: Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<u64, AlcedoError> {
        let pk = get_pk_key(&self.app_state.database_schema, &self.collection).await?;
        let mut query = Query {
            ..Default::default()
        };

        let mut hmap = FieldFilter {
            fields: HashMap::new(),
        };

        hmap.fields.insert(
            pk.name,
            FieldValue::Comparison(Comparison {
                _in: Some(pks.into()),
                ..Default::default()
            }),
        );
        query.filter = LogicOp {
            _and: Some(vec![Filter::Field(hmap)]),
            _or: None,
        };
        self.delete_items_by_query(query, &mut database_transaction)
            .await
    }

    pub async fn delete_items_by_query<'a>(
        &self,
        query: Query,
        database_transaction: &'a mut Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<u64, AlcedoError> {
        match database_transaction {
            Some(existing_tx) => self.delete_items_by_query_with_tx(existing_tx, query).await,
            None => {
                let mut tx = self.app_state.database_pool.begin().await?;
                let result = self.delete_items_by_query_with_tx(&mut tx, query).await?;
                tx.commit().await?;
                Ok(result)
            }
        }
    }

    pub async fn delete_items_by_query_with_tx<'a>(
        &self,
        tx: &'a mut Transaction<'_, Postgres>,
        mut query: Query,
    ) -> Result<u64, AlcedoError> {
        let hook_context = HookContext {
            context: self.app_context.clone(),
            state: self.app_state.clone(),
            tx,
        };

        let pk_name = get_pk_key(&self.app_state.database_schema, &self.collection)
            .await?
            .name;

        query.fields = vec![pk_name.clone()];

        let get_pks = query
            .execute_query(self.app_context, self.app_state, self.collection)
            .await?;

        let get_pks = get_pks
            .clone()
            .iter()
            .map(|item| item.get(&pk_name).unwrap().clone())
            .collect();

        let mut before = ItemsBeforeDelete {
            keys: get_pks,
            collection: self.collection.clone(),
        };

        self.app_state
            .event_bus
            .trigger(
                &format!("before.items.create.{}", self.collection),
                &mut before,
                hook_context,
            )
            .await;

        let queries: Vec<Result<String, AlcedoError>> = join_all(
            before
                .keys
                .iter()
                .map(|pk| self.generate_delete_item_query(pk)),
        )
        .await;

        let errors: Vec<String> = queries
            .iter()
            .filter_map(|query| query.as_ref().err())
            .map(|err_ref| err_ref.to_string())
            .collect();

        if errors.len() > 0 {
            return Err(AlcedoError::InvalidInput(errors.join(","), 1));
        };

        let mut total_deleted = 0;
        for query in queries {
            let query = query.unwrap();

            let insert = sqlx::query(&query).execute(&mut **tx).await;
            let result = match insert {
                Err(e) => {
                    let e = process_query_error_response(e, &query);
                    println!("{:?} {}", e, query);
                    return Err(e);
                }
                Ok(e) => e,
            };
            total_deleted += result.rows_affected();
        }
        let mut after = ItemsAfterDelete {
            keys: before.keys,
            collection: self.collection.clone(),
        };

        let hook_context = HookContext {
            context: self.app_context.clone(),
            state: self.app_state.clone(),
            tx,
        };
        self.app_state
            .event_bus
            .trigger(
                &format!("after.items.delete.{}", self.collection),
                &mut after,
                hook_context,
            )
            .await;

        Ok(total_deleted)
    }

    async fn generate_insert_item_query(
        &self,
        item: Map<String, Value>,
    ) -> Result<String, AlcedoError> {
        let mut stmt = sea_query::Query::insert();

        stmt.into_table((
            Alias::new(self.app_context.schema_name()),
            Alias::new(self.collection),
        ));

        let mut columns = vec![];
        let mut values = vec![];
        let schema = self.app_state.database_schema.read().await;
        for (field, value) in item.clone() {
            let column = schema.columns.iter().find(|col| {
                &col.table == self.collection
                    && col.name == field
                    && col.schema == self.app_context.schema_name()
            });

            if let None = column {
                return Err(AlcedoError::InvalidInput(
                    format!("Unknown field {} provided", field),
                    1,
                ));
            }
            let value = match parse_value(&schema, &vec![], &self.collection, &field, value) {
                None => {
                    if let Some(column) = column {
                        if column.is_nullable || column.has_auto_increment {
                            continue;
                        }
                    }
                    return Err(AlcedoError::InvalidInput(
                        format!("Invalid type provided for field {}", field),
                        1,
                    ));
                }
                Some(val) => val,
            };

            columns.push(field);
            values.push(value);
        }
        let pk = get_pk_key(&self.app_state.database_schema, &self.collection).await?;
        stmt.columns(columns.iter().map(|col| Alias::new(col)));
        stmt.returning_col(Alias::new(pk.name.clone()));
        let _ = stmt.values(values);

        Ok(stmt.to_string(PostgresQueryBuilder))
    }

    async fn generate_update_item_query(
        &self,
        item: &Map<String, Value>,
        pk_val: Value,
    ) -> Result<String, AlcedoError> {
        let mut stmt = sea_query::Query::update();

        stmt.table((
            Alias::new(self.app_context.schema_name()),
            Alias::new(self.collection),
        ));

        let mut values: Vec<(Alias, SimpleExpr)> = vec![];

        let schema = self.app_state.database_schema.read().await;
        for (field, value) in item.clone() {
            let column = schema.columns.iter().find(|col| {
                &col.table == self.collection
                    && col.name == field
                    && col.schema == self.app_context.schema_name()
            });
            if let None = column {
                return Err(AlcedoError::InvalidInput(
                    format!("Unknown field {} provided", field),
                    1,
                ));
            }
            let schema = self.app_state.database_schema.read().await;
            let value = match parse_value(&schema, &vec![], &self.collection, &field, value) {
                None => {
                    let is_nullable = column.map_or(false, |c| c.is_nullable);

                    if is_nullable {
                        let val: Option<i32> = None;
                        let expr = Expr::value(val);
                        expr
                    } else {
                        return Err(AlcedoError::InvalidInput(
                            format!("Field {} is not nullable and received invalid input", field),
                            1,
                        ));
                    }
                }
                Some(val) => val,
            };

            values.push((Alias::new(field), value));
        }
        let pk = get_pk_key(&self.app_state.database_schema, &self.collection).await?;
        stmt.returning_col(Alias::new(pk.name.clone()));
        stmt.values(values);
        stmt.and_where(Expr::eq(Expr::col(Alias::new(pk.name.clone())), pk_val));

        Ok(stmt.to_string(PostgresQueryBuilder))
    }

    async fn generate_delete_item_query(&self, pk_val: &Value) -> Result<String, AlcedoError> {
        let mut stmt = sea_query::Query::delete();

        stmt.from_table((
            Alias::new(self.app_context.schema_name()),
            Alias::new(self.collection),
        ));

        let pk = get_pk_key(&self.app_state.database_schema, &self.collection).await?;
        stmt.and_where(Expr::eq(
            Expr::col(Alias::new(pk.name.clone())),
            pk_val.clone(),
        ));
        Ok(stmt.to_string(PostgresQueryBuilder))
    }
}

#[cfg(test)]
mod tests {
    // use std::{collections::HashMap, hash::Hash};

    // use sea_query::table;

    // use crate::utils;

    // use super::*;

    #[tokio::test]
    async fn test_create_table() {
        // let state = utils::test_utils::get_app_state().await;

        // let mut hmap = FieldFilter {
        //     fields: HashMap::new(),
        //     // extra_filters: HashMap::new(),
        // };
        // hmap.fields.insert(
        //     "field1".to_string(),
        //     Comparison {
        //         _contains: None,
        //         _eq: Some("Test".to_string()),
        //     },
        // );
        // let mut hmap1 = FieldFilter {
        //     fields: HashMap::new(),
        //     // extra_filters: HashMap::new(),
        // };
        // hmap1.fields.insert(
        //     "field1".to_string(),
        //     Comparison {
        //         _contains: None,
        //         _eq: Some("Test".to_string()),
        //     },
        // );
        // let mut hmap2 = FieldFilter {
        //     fields: HashMap::new(),
        //     // extra_filters: HashMap::new(),
        // };
        // hmap2.fields.insert(
        //     "field1".to_string(),
        //     Comparison {
        //         _contains: None,
        //         _eq: Some("Test3".to_string()),
        //     },
        // );
        // let query = Query {
        //     fields: vec!["field1".to_string(), "field2".to_string()],
        //     sort: vec!["+field1".to_string()],
        //     filter: LogicOp {
        //         _and: Some(vec![
        //             Filter::Field(hmap),
        //             Filter::Logic(LogicOp {
        //                 _and: None,
        //                 _or: Some(vec![Filter::Logic(LogicOp {
        //                     _and: Some(vec![Filter::Field(hmap1), Filter::Field(hmap2)]),
        //                     _or: None,
        //                 })]),
        //             }),
        //         ]),
        //         _or: None,
        //     },
        // };
        // println!("{}", query.to_string());
        ()
    }
}
