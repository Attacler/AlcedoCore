use std::collections::HashMap;

use futures::future::join_all;
use sea_query::{Alias, Expr, PostgresQueryBuilder, SimpleExpr};
use serde_json::{Map, Value};
use sqlx::{Postgres, Transaction};

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_common::error::AppError;
use alcedo_common::state::CoreState;

use crate::services::items::jsonvalue_simpleexpr::parse_value;
use crate::services::items::query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query};
use crate::services::tables::get_pk_key;

pub struct ItemsService<'a> {
    core: &'a CoreState,
    collection: &'a String,
    app_context: AppContext,
}

impl ItemsService<'_> {
    pub fn new<'a>(
        core: &'a CoreState,
        context: &AppContext,
        collection: &'a String,
    ) -> ItemsService<'a> {
        return ItemsService {
            core,
            collection,
            app_context: context.clone(),
        };
    }

    /// Service bound to the global `alcedo` schema with the API request source,
    /// for registry tables that live outside any app×version schema.
    pub fn for_global<'a>(core: &'a CoreState, collection: &'a String) -> ItemsService<'a> {
        ItemsService {
            core,
            collection,
            app_context: AppContext {
                app_name: crate::db::ALCEDO_SCHEMA.to_string(),
                version: String::new(),
                request_source: RequestSource::API,
            },
        }
    }

    pub async fn read_items_by_query(
        &self,
        mut query: Query,
    ) -> Result<Vec<Map<String, Value>>, AppError> {
        Ok(query
            .execute_query(&self.app_context, self.core, self.collection)
            .await?)
    }

    pub async fn read_list(
        &self,
        pool: &crate::db::Pool,
        request: crate::services::items::read::ListRequest,
    ) -> Result<crate::services::items::read::ListResult, AppError> {
        crate::services::items::read::execute_list(pool, self.collection, request).await
    }

    pub async fn read_one(
        &self,
        pool: &crate::db::Pool,
        request: crate::services::items::read::OneRequest,
    ) -> Result<Option<serde_json::Value>, AppError> {
        crate::services::items::read::execute_one(pool, self.collection, request).await
    }

    /// List a physical or collection-backed table addressed by `reference`.
    ///
    /// The collection the service was constructed with is ignored for these
    /// calls. The caller is responsible for authorization/allowlisting: the
    /// engine only applies permissions present on the request, so this method
    /// must not be exposed to caller-influenced [`crate::services::items::shape::TableRef`]s
    /// without boundary checks.
    pub async fn read_list_for_table(
        &self,
        pool: &crate::db::Pool,
        reference: crate::services::items::shape::TableRef,
        request: crate::services::items::read::ListRequest,
    ) -> Result<crate::services::items::read::ListResult, AppError> {
        let shape = crate::services::items::shape::TableShape::resolve(
            self.core,
            &self.app_context,
            reference,
        )
        .await?;
        crate::services::items::read::execute_list_for_table(pool, &shape, request).await
    }

    /// Fetch one row from the table addressed by `reference`.
    ///
    /// The collection the service was constructed with is ignored for these
    /// calls. The caller is responsible for authorization/allowlisting: the
    /// engine only applies permissions present on the request, so this method
    /// must not be exposed to caller-influenced [`crate::services::items::shape::TableRef`]s
    /// without boundary checks.
    pub async fn read_one_for_table(
        &self,
        pool: &crate::db::Pool,
        reference: crate::services::items::shape::TableRef,
        request: crate::services::items::read::OneRequest,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let shape = crate::services::items::shape::TableShape::resolve(
            self.core,
            &self.app_context,
            reference,
        )
        .await?;
        crate::services::items::read::execute_one_for_table(pool, &shape, request).await
    }

    pub async fn read_grouped(
        &self,
        pool: &crate::db::Pool,
        request: crate::db::filter_condition::GroupedQueryRequest,
        permissions: &[crate::services::permissions::PolicyPermission],
    ) -> Result<crate::db::filter_condition::GroupedQueryResponse, AppError> {
        crate::services::items::read::execute_grouped(pool, self.collection, request, permissions)
            .await
    }

    /// Group the table addressed by `reference`.
    ///
    /// The collection the service was constructed with is ignored for these
    /// calls. The caller is responsible for authorization/allowlisting: the
    /// engine only applies permissions present on the request, so this method
    /// must not be exposed to caller-influenced
    /// [`crate::services::items::shape::TableRef`]s without boundary checks.
    pub async fn read_grouped_for_table(
        &self,
        pool: &crate::db::Pool,
        reference: crate::services::items::shape::TableRef,
        request: crate::db::filter_condition::GroupedQueryRequest,
        permissions: &[crate::services::permissions::PolicyPermission],
    ) -> Result<crate::db::filter_condition::GroupedQueryResponse, AppError> {
        let shape = crate::services::items::shape::TableShape::resolve(
            self.core,
            &self.app_context,
            reference,
        )
        .await?;
        crate::services::items::read::execute_grouped_for_table(pool, &shape, request, permissions)
            .await
    }

    pub async fn read_references(
        &self,
        pool: &crate::db::Pool,
        item_id: &str,
        collection_filters: &std::collections::HashMap<
            String,
            Vec<crate::services::permissions::PolicyPermission>,
        >,
    ) -> Result<Vec<crate::db::collections::ReferencingGroup>, AppError> {
        crate::services::items::read::execute_references(
            pool,
            self.collection,
            item_id,
            collection_filters,
        )
        .await
    }

    /// Resolve the write shape for `reference`, widening the writable allowlist
    /// with server-managed columns the boundary is trusted to set (e.g.
    /// `password_hash`). Used only by privileged `alcedo-api` write handlers; the
    /// engine's default deny posture is preserved for everyone else.
    pub async fn privileged_write_shape(
        &self,
        reference: crate::services::items::shape::TableRef,
        extra_writable: &[&str],
    ) -> Result<crate::services::items::shape::TableShape, AppError> {
        let mut shape = crate::services::items::shape::TableShape::resolve(
            self.core,
            &self.app_context,
            reference,
        )
        .await?;
        if let Some(writable) = shape.writable.as_mut() {
            for col in extra_writable {
                if !writable.iter().any(|c| c == col) {
                    writable.push(col.to_string());
                }
            }
        }
        Ok(shape)
    }

    /// Resolve the current collection name as a [`TableShape`] for write
    /// dispatch. Returns `None` when the name is not materialized in the
    /// schema, so the metadata-driven collection path stays in charge.
    async fn resolve_write_shape(
        &self,
    ) -> Result<Option<crate::services::items::shape::TableShape>, AppError> {
        use crate::services::items::shape::{TableRef, TableShape};

        match TableShape::resolve(
            self.core,
            &self.app_context,
            TableRef {
                schema: None,
                name: self.collection.clone(),
            },
        )
        .await
        {
            Ok(shape) => Ok(Some(shape)),
            Err(AppError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn create(
        &self,
        pool: &crate::db::Pool,
        body: crate::db::collection_items::CreateItemsBody,
    ) -> Result<crate::services::items::write::WriteOutcome, AppError> {
        if let Some(shape) = self.resolve_write_shape().await? {
            if shape.collection.is_none() {
                let items = match &body {
                    crate::db::collection_items::CreateItemsBody::Single(item) => {
                        vec![item.clone()]
                    }
                    crate::db::collection_items::CreateItemsBody::Multiple(items) => items.clone(),
                };
                return crate::services::items::write::execute_create_for_table(pool, &shape, items)
                    .await;
            }
        }

        crate::services::items::write::execute_create(pool, self.collection, body).await
    }

    pub async fn update(
        &self,
        pool: &crate::db::Pool,
        body: crate::db::collection_items::UpdateItemsBody,
        perm_filter: Option<(String, Vec<serde_json::Value>)>,
    ) -> Result<crate::services::items::write::WriteOutcome, AppError> {
        if let Some(shape) = self.resolve_write_shape().await? {
            if shape.collection.is_none() {
                let filter: crate::db::filter_condition::FilterCondition =
                    serde_json::from_value(body.filter.clone())
                        .map_err(|e| AppError::BadRequest(format!("Invalid filter: {}", e)))?;
                return crate::services::items::write::execute_bulk_update_for_table(
                    pool, &shape, filter, &body.update, perm_filter,
                )
                .await;
            }
        }
        crate::services::items::write::execute_update(pool, self.collection, body, perm_filter).await
    }

    pub async fn update_one(
        &self,
        pool: &crate::db::Pool,
        id: &str,
        body: &Map<String, Value>,
        permissions: &[crate::services::permissions::PolicyPermission],
    ) -> Result<crate::services::items::write::WriteOutcome, AppError> {
        if let Some(shape) = self.resolve_write_shape().await? {
            if shape.collection.is_none() {
                let pk_value = Value::String(id.to_string());
                return crate::services::items::write::execute_update_one_for_table(
                    pool, &shape, &pk_value, body,
                )
                .await;
            }
        }

        crate::services::items::write::execute_update_one(
            pool,
            self.collection,
            id,
            body,
            permissions,
        )
        .await
    }

    pub async fn delete(
        &self,
        pool: &crate::db::Pool,
        body: crate::db::collection_items::DeleteItemsBody,
        perm_filter: Option<(String, Vec<serde_json::Value>)>,
    ) -> Result<crate::services::items::write::WriteOutcome, AppError> {
        if let Some(shape) = self.resolve_write_shape().await? {
            if shape.collection.is_none() {
                // Physical-table deletes intentionally do not apply `perm_filter`
                // (Phase 5 system handlers pass None; boundary checks upstream).
                // It is not forwarded to `execute_delete_for_table*`.
                return match (&body.filter, &body.pk_values) {
                    (None, Some(pks)) => {
                        crate::services::items::write::execute_delete_for_table(pool, &shape, pks.clone())
                            .await
                    }
                    (Some(filter), None) => {
                        let filter: crate::db::filter_condition::FilterCondition =
                            serde_json::from_value(filter.clone())
                                .map_err(|e| AppError::BadRequest(format!("Invalid filter: {}", e)))?;
                        crate::services::items::write::execute_delete_for_table_by_filter(pool, &shape, filter)
                            .await
                    }
                    _ => Err(AppError::BadRequest(
                        "Table deletes require pk_values OR a filter".to_string(),
                    )),
                };
            }
        }
        crate::services::items::write::execute_delete(pool, self.collection, body, perm_filter).await
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
            .execute_query(&self.app_context, self.core, self.collection)
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
            .execute_query(&self.app_context, self.core, self.collection)
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
        stmt.columns(columns.iter().map(|col| Alias::new(col)));
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
