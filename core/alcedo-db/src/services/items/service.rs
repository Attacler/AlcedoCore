use std::collections::HashMap;

use futures::future::join_all;
use sea_query::{Alias, Expr, PostgresQueryBuilder, SimpleExpr};
use serde_json::{Map, Value};
use sqlx::{Postgres, Transaction};

use alcedo_common::context::AppContext;
use alcedo_common::error::AppError;
use alcedo_common::state::CoreState;

use crate::services::items::jsonvalue_simpleexpr::parse_value;
use crate::services::items::query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query};
use crate::services::tables::get_pk_key;

pub struct ItemsService<'a> {
    core: &'a CoreState,
    collection: &'a String,
    app_context: &'a AppContext,
}

impl ItemsService<'_> {
    pub fn new<'a>(
        core: &'a CoreState,
        context: &'a AppContext,
        collection: &'a String,
    ) -> ItemsService<'a> {
        return ItemsService {
            core,
            collection,
            app_context: context,
        };
    }

    pub async fn read_items_by_query(
        &self,
        mut query: Query,
    ) -> Result<Vec<Map<String, Value>>, AppError> {
        Ok(query
            .execute_query(self.app_context, self.core, self.collection)
            .await?)
    }

    pub async fn update_items_by_query<'a>(
        &self,
        query: &mut Query,
        payload: Map<String, Value>,
        database_transaction: &'a mut Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<Vec<String>, AppError> {
        match database_transaction {
            Some(existing_tx) => self.update_items_with_tx(existing_tx, query, payload).await,
            None => {
                let mut tx = self.core.pool()?.begin().await?;
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
    ) -> Result<Vec<String>, AppError> {
        let pk_name = get_pk_key(&self.core.schema, self.collection.as_str())
            .await?
            .name;

        query.fields = vec![pk_name.clone()];

        let get_pks = query
            .execute_query(self.app_context, self.core, self.collection)
            .await?;

        let queries: Vec<Result<String, AppError>> = join_all(get_pks.iter().map(|item| {
            self.generate_update_item_query(&payload, item.get(&pk_name).cloned().unwrap())
        }))
        .await;

        let errors: Vec<String> = queries
            .iter()
            .filter_map(|query| query.as_ref().err())
            .map(|err_ref| err_ref.to_string())
            .collect();

        if !errors.is_empty() {
            return Err(AppError::BadRequest(errors.join(",")));
        }

        let mut update_items: Vec<String> = vec![];
        for query in queries {
            let query_str = query.unwrap();
            let result = execute_query_transaction(tx, &query_str).await?;
            let pk_data = pgrow_to_json(result.get(0).unwrap()).unwrap();
            let pk = pk_data.get(&pk_name).unwrap().to_string();
            update_items.push(pk);
        }

        Ok(update_items)
    }

    pub async fn create_many<'a>(
        &self,
        items: Vec<Map<String, Value>>,
        database_transaction: &'a mut Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<Vec<String>, AppError> {
        match database_transaction {
            Some(existing_tx) => self.create_items_with_tx(existing_tx, items).await,
            None => {
                let mut tx = self.core.pool()?.begin().await?;
                let result = self.create_items_with_tx(&mut tx, items).await?;
                tx.commit().await?;
                Ok(result)
            }
        }
    }

    async fn create_items_with_tx<'a>(
        &self,
        tx: &'a mut Transaction<'_, Postgres>,
        mut items: Vec<Map<String, Value>>,
    ) -> Result<Vec<String>, AppError> {
        let pk_name = get_pk_key(&self.core.schema, self.collection.as_str())
            .await?
            .name;
        let pk_name_cloned = pk_name.clone();

        let queries: Vec<Result<String, AppError>> = join_all(
            items
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
            return Err(AppError::BadRequest(errors.join(",")));
        }

        let mut created_items = vec![];
        for query in queries {
            let query = query.unwrap();
            let result = execute_query_transaction(tx, &query).await?;
            let pk = pgrow_to_json(result.get(0).unwrap()).unwrap();
            let pk = pk.get(&pk_name).unwrap().to_string();

            created_items.push(pk);
        }

        for (item, pk) in items.iter_mut().zip(created_items.iter()) {
            item.insert(pk_name_cloned.clone(), Value::String(pk.clone()));
        }

        Ok(created_items)
    }

    pub async fn delete_items_by_pks<'a>(
        &self,
        pks: Vec<Value>,
        mut database_transaction: Option<&mut Transaction<'_, Postgres>>,
    ) -> Result<u64, AppError> {
        let pk = get_pk_key(&self.core.schema, self.collection.as_str()).await?;
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
    ) -> Result<u64, AppError> {
        match database_transaction {
            Some(existing_tx) => self.delete_items_by_query_with_tx(existing_tx, query).await,
            None => {
                let mut tx = self.core.pool()?.begin().await?;
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
    ) -> Result<u64, AppError> {
        let pk_name = get_pk_key(&self.core.schema, self.collection.as_str())
            .await?
            .name;

        query.fields = vec![pk_name.clone()];

        let get_pks = query
            .execute_query(self.app_context, self.core, self.collection)
            .await?;

        let pks: Vec<Value> = get_pks
            .clone()
            .iter()
            .map(|item| item.get(&pk_name).cloned().unwrap())
            .collect();

        let queries: Vec<Result<String, AppError>> =
            join_all(pks.iter().map(|pk| self.generate_delete_item_query(pk))).await;

        let errors: Vec<String> = queries
            .iter()
            .filter_map(|query| query.as_ref().err())
            .map(|err_ref| err_ref.to_string())
            .collect();

        if errors.len() > 0 {
            return Err(AppError::BadRequest(errors.join(",")));
        }

        let mut total_deleted = 0;
        for query in queries {
            let query = query.unwrap();
            let rows = execute_query_transaction(tx, &query).await?;
            total_deleted += rows.len();
        }

        Ok(total_deleted as u64)
    }

    async fn generate_insert_item_query(
        &self,
        item: Map<String, Value>,
    ) -> Result<String, AppError> {
        let mut stmt = sea_query::Query::insert();

        stmt.into_table((
            Alias::new(self.app_context.schema_name()),
            Alias::new(self.collection),
        ));

        let mut columns = vec![];
        let mut values = vec![];
        let schema = self.core.schema.read().await.clone();
        for (field, value) in item.clone() {
            let column = schema.columns.iter().find(|col| {
                &col.table == self.collection
                    && col.name == field
                    && col.schema == self.app_context.schema_name()
            });

            if let None = column {
                return Err(AppError::BadRequest(format!(
                    "Unknown field {} provided",
                    field
                )));
            }
            let value = match parse_value(
                &schema,
                &vec![],
                self.collection.as_str(),
                field.as_str(),
                value,
            ) {
                None => {
                    if let Some(column) = column {
                        if column.is_nullable || column.has_auto_increment {
                            continue;
                        }
                    }
                    return Err(AppError::BadRequest(format!(
                        "Invalid type provided for field {}",
                        field
                    )));
                }
                Some(val) => val,
            };

            columns.push(field);
            values.push(value);
        }
        let pk = get_pk_key(&self.core.schema, self.collection.as_str()).await?;
        stmt.columns(columns.iter().map(|col| Alias::new(col)));
        stmt.returning_col(Alias::new(pk.name.clone()));
        let _ = stmt.values(values);

        let sql = stmt.to_string(PostgresQueryBuilder);
        Ok(format!(
            "{} RETURNING row_to_json(\"{}\".\"{}\".*)",
            sql,
            self.app_context.schema_name(),
            self.collection
        ))
    }

    async fn generate_update_item_query(
        &self,
        item: &Map<String, Value>,
        pk_val: Value,
    ) -> Result<String, AppError> {
        let mut stmt = sea_query::Query::update();

        stmt.table((
            Alias::new(self.app_context.schema_name()),
            Alias::new(self.collection),
        ));

        let mut values: Vec<(Alias, SimpleExpr)> = vec![];

        let schema = self.core.schema.read().await.clone();
        for (field, value) in item.clone() {
            let column = schema.columns.iter().find(|col| {
                &col.table == self.collection
                    && col.name == field
                    && col.schema == self.app_context.schema_name()
            });
            if let None = column {
                return Err(AppError::BadRequest(format!(
                    "Unknown field {} provided",
                    field
                )));
            }
            let value = match parse_value(
                &schema,
                &vec![],
                self.collection.as_str(),
                field.as_str(),
                value,
            ) {
                None => {
                    let is_nullable = column.map_or(false, |c| c.is_nullable);

                    if is_nullable {
                        let val: Option<i32> = None;
                        let expr = Expr::value(val);
                        expr
                    } else {
                        return Err(AppError::BadRequest(format!(
                            "Field {} is not nullable and received invalid input",
                            field
                        )));
                    }
                }
                Some(val) => val,
            };

            values.push((Alias::new(field), value));
        }
        let pk = get_pk_key(&self.core.schema, self.collection.as_str()).await?;
        stmt.returning_col(Alias::new(pk.name.clone()));
        stmt.values(values);
        stmt.and_where(Expr::eq(
            Expr::col(Alias::new(pk.name.clone())),
            parse_value(
                &schema,
                &vec![],
                self.collection.as_str(),
                pk.name.as_str(),
                pk_val,
            )
            .ok_or_else(|| AppError::BadRequest("Invalid primary key value".to_string()))?,
        ));

        let sql = stmt.to_string(PostgresQueryBuilder);
        Ok(format!(
            "{} RETURNING row_to_json(\"{}\".\"{}\".*)",
            sql,
            self.app_context.schema_name(),
            self.collection
        ))
    }

    async fn generate_delete_item_query(&self, pk_val: &Value) -> Result<String, AppError> {
        let mut stmt = sea_query::Query::delete();

        stmt.from_table((
            Alias::new(self.app_context.schema_name()),
            Alias::new(self.collection),
        ));

        let pk = get_pk_key(&self.core.schema, self.collection.as_str()).await?;
        let schema = self.core.schema.read().await.clone();
        stmt.and_where(Expr::eq(
            Expr::col(Alias::new(pk.name.clone())),
            parse_value(
                &schema,
                &vec![],
                self.collection.as_str(),
                pk.name.as_str(),
                pk_val.clone(),
            )
            .ok_or_else(|| AppError::BadRequest("Invalid primary key value".to_string()))?,
        ));
        let sql = stmt.to_string(PostgresQueryBuilder);
        Ok(format!(
            "{} RETURNING row_to_json(\"{}\".\"{}\".*)",
            sql,
            self.app_context.schema_name(),
            self.collection
        ))
    }
}

/// Execute a DML statement inside the provided transaction and return the
/// affected rows as JSON values (the statement ends with a
/// `RETURNING row_to_json(...)` wrapper).
async fn execute_query_transaction(
    tx: &mut Transaction<'_, Postgres>,
    sql: &str,
) -> Result<Vec<Value>, AppError> {
    let q = sqlx::query_as::<_, (Value,)>(sql);
    let rows: Vec<(Value,)> =
        q.fetch_all(&mut **tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Query execution failed: {}", e),
            })?;
    Ok(rows
        .into_iter()
        .filter_map(|(row,)| row.as_object().map(|obj| Value::Object(obj.clone().into())))
        .collect())
}

/// Convert the first decoded row to a JSON value object.
fn pgrow_to_json(row: &Value) -> Option<Value> {
    Some(row.clone())
}
