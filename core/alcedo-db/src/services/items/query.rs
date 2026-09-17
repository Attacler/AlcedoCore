use sea_query::{
    extension::postgres::PgExpr, Alias, Condition, Expr, JoinType, Order, PostgresQueryBuilder,
    SelectStatement, SimpleExpr,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::vec;
use utoipa::ToSchema;
use uuid::Uuid;

use alcedo_common::context::AppContext;
use alcedo_common::error::AppError;
use alcedo_common::state::{CoreDatabaseSchema, CoreState};

use crate::db::Pool;
use crate::db::collections::CollectionDefinition;
use crate::db::field_resolver::{detect_direction, Direction};
use crate::services::items::jsonvalue_simpleexpr::parse_value;

const LIMIT_DEFAULT: u64 = 200;

fn limit_default() -> u64 {
    LIMIT_DEFAULT
}

/// Execute the generated SQL and decode the result as JSON objects,
/// matching the wrapping used by `db::collection_items` / `db::query_builder`
/// (`COALESCE(json_agg(...), '[]'::json)`).
async fn execute_sql(pool: &Pool, sql: &str) -> Result<Vec<Map<String, Value>>, AppError> {
    let wrapped = format!(
        "SELECT COALESCE(json_agg(\"_q\"), '[]'::json) FROM ({}) AS \"_q\"",
        sql.trim_end_matches(';')
    );

    let (result,): (Value,) = sqlx::query_as::<_, (Value,)>(&wrapped)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Query execution failed: {}", e),
        })?;

    let items: Vec<Value> = match result {
        Value::Array(arr) => arr,
        _ => vec![],
    };

    Ok(items
        .iter()
        .filter_map(|item| item.as_object().cloned())
        .collect())
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Query {
    #[serde(default)]
    pub joins: Vec<TableJoin>,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub sort: Vec<String>,
    #[serde(default)]
    pub filter: LogicOp,
    #[serde(default = "limit_default")]
    pub limit: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemainingQuery {
    pub table: String,
    pub fields: Vec<String>,
    /// Base-row key used to link related rows: the FK column for M:1, the base
    /// PK column for 1:M.
    pub fk: String,
    /// Output key for the expanded relation (the relation field name).
    pub alias: String,
    /// `Some(target_fk)` marks a 1:M relation: the column on the target table
    /// that references the base PK.
    pub fk_on_target: Option<String>,
    pub app_context: AppContext,
}

impl Query {
    /// Executes the Query object as a sql query on the database
    ///
    /// * `state` - The CoreState
    /// * `table` - The target table
    pub async fn execute_query(
        &mut self,
        context: &AppContext,
        state: &CoreState,
        table: &String,
    ) -> Result<Vec<Map<String, Value>>, AppError> {
        let schema = state.schema.read().await.clone();
        let requested_fields = self.fields.clone();
        let pool = state.pool()?;

        // Collection definitions let the engine tell M:1 from 1:M. 1:M fields
        // are virtual (no FK column exists on the base table), so direction can
        // only be derived from the collection metadata. Absent outside an
        // app×version schema (e.g. migration contexts), hence the default.
        let all_collections = crate::db::collections::list_collections(pool)
            .await
            .unwrap_or_default();

        let base_pk = schema
            .columns
            .iter()
            .find(|c| c.table == *table && c.is_primary_key)
            .map(|c| c.name.clone());

        let (stmt, remaining_queries) =
            self.to_sql(table.as_str(), &schema, context, &all_collections)?;
        // Because we can have relations (for example fields[]=customer.name) we will have a query that will need to execute after this one aka "remaining"
        let query_str = stmt.to_string(PostgresQueryBuilder);

        let mut rows = execute_sql(pool, &query_str).await?;

        let has_one_to_many = remaining_queries.iter().any(|rq| rq.fk_on_target.is_some());

        // Lets execute the remaining queries based on the fk`s that came from our "base" query
        for rq in remaining_queries {
            let fk_vals: Vec<Value> = rows
                .iter()
                .filter_map(|row| row.get(&rq.fk))
                .cloned()
                .collect();

            if fk_vals.is_empty() {
                continue;
            }

            let pk_name = match schema
                .columns
                .iter()
                .find(|c| c.table == rq.table && c.is_primary_key)
            {
                Some(v) => v.name.clone(),
                None => continue,
            };

            // M:1 links on the target PK; 1:M links on the target FK column.
            let (link_field, mut nested_fields) = match &rq.fk_on_target {
                Some(target_fk) => (target_fk.clone(), rq.fields.clone()),
                None => (pk_name.clone(), rq.fields.clone()),
            };
            if !nested_fields.iter().any(|f| f == &link_field) {
                nested_fields.push(link_field.clone());
            }

            // lets create a new Query object to fetch the relations
            // we do this by a simple where link_field in (id1,id2 ect)
            let mut field_filter = HashMap::new();
            field_filter.insert(
                link_field,
                FieldValue::Comparison(Comparison {
                    _in: Some(Value::Array(fk_vals)),
                    ..Default::default()
                }),
            );

            let filter = LogicOp {
                _and: Some(vec![Filter::Field(FieldFilter {
                    fields: field_filter,
                })]),
                _or: None,
            };

            let mut nested_query = Query {
                fields: nested_fields,
                filter,
                joins: vec![],
                sort: vec![],
                limit: 0,
            };

            let related_rows =
                Box::pin(nested_query.execute_query(&rq.app_context, state, &rq.table)).await?;

            match &rq.fk_on_target {
                Some(target_fk) => {
                    // 1:M: group related rows into an array under the relation key.
                    for row in rows.iter_mut() {
                        let Some(key) = row.get(&rq.fk).cloned() else {
                            continue;
                        };
                        let matches: Vec<Value> = related_rows
                            .iter()
                            .filter(|rr| rr.get(target_fk) == Some(&key))
                            .map(|rr| {
                                let mut obj = rr.clone();
                                // The link column was only fetched for grouping.
                                obj.remove(target_fk);
                                Value::Object(obj)
                            })
                            .collect();
                        row.insert(rq.alias.clone(), Value::Array(matches));
                    }
                }
                None => {
                    // M:1: replace the FK value with the related object.
                    for row in rows.iter_mut() {
                        let Some(val) = row.get(&rq.fk).cloned() else {
                            continue;
                        };
                        let related_value = related_rows
                            .iter()
                            .find(|row| {
                                row.get(&pk_name).unwrap_or(&Value::String("".to_string())) == &val
                            })
                            .cloned();

                        if let Some(value) = related_value {
                            row.insert(rq.alias.clone(), Value::Object(value));
                        }
                    }
                }
            }
        }

        // `to_sql` adds the base PK so 1:M grouping has a key to link on; drop
        // it again when the caller did not request it.
        if has_one_to_many {
            if let Some(pk) = &base_pk {
                let requested = requested_fields.is_empty()
                    || requested_fields.iter().any(|f| f == pk || f == "*");
                if !requested {
                    for row in rows.iter_mut() {
                        row.remove(pk);
                    }
                }
            }
        }

        Ok(rows)
    }

    /// Converts the Query into a sea_query select statement
    ///
    /// * `table` - The target table
    /// * `schema` - The database schema
    pub fn to_sql(
        &mut self,
        table: &str,
        schema: &CoreDatabaseSchema,
        context: &AppContext,
        all_collections: &[CollectionDefinition],
    ) -> Result<(SelectStatement, Vec<RemainingQuery>), AppError> {
        let table_schema = schema
            .tables
            .iter()
            .find(|t| t.name == table && t.schema == context.schema_name());
        if let None = table_schema {
            return Err(AppError::NotFound(format!(
                "Collection '{}' not found",
                table
            )));
        }
        // because theoreticly someone could provide the joins trough the API, we will always overwrite this
        self.joins = vec![];
        let mut stmt = sea_query::Query::select();

        stmt.from((Alias::new(context.schema_name()), Alias::new(table)));

        if self.limit != 0 {
            stmt.limit(self.limit);
        }
        // if there are no fields give, we assume that all values will need to be returned
        if self.fields.is_empty() {
            let table_fields = schema
                .columns
                .iter()
                .filter(|f| f.table == table && f.schema == context.schema_name())
                .map(|f| format!("{}", f.name))
                .collect();
            self.fields = table_fields;
        } else {
            let mut fields = self.fields.clone();

            // if there is a * provided, we will need to make sure that all fields are returned
            while let Some(index) = fields.iter().position(|f| f.to_string() == "*".to_string()) {
                fields.remove(index);

                schema
                    .columns
                    .iter()
                    .filter(|f| f.table == table && f.schema == context.schema_name())
                    .map(|f| format!("{}", f.name))
                    .for_each(|f| fields.push(f));
            }
            self.fields = fields;
        }

        let fields = &self.fields.clone();

        let mut related_fields: Vec<RemainingQuery> = vec![];
        let base_def = all_collections.iter().find(|c| c.name == table);

        // based on the provided fields, we should fill the related_fields vector
        for field in fields {
            // A dot-notation field is a relation. M:1 relations have a physical
            // FK column on the base table; 1:M relations are virtual and are
            // resolved from the collection metadata below.
            if field.contains(".") {
                let relation_field = field.split(".").next().unwrap();
                let len = relation_field.len() + 1;
                let remaining: String = field.chars().skip(len).take(field.len() - len).collect();

                let fk_column = schema.columns.iter().find(|col| {
                    col.table == table
                        && col.name == relation_field
                        && col.schema == context.schema_name()
                });

                match fk_column.and_then(|col| col.foreign_key.clone()) {
                    // M:1 / 1:1 — FK lives on the base table.
                    Some(fk_info) => {
                        let find = related_fields
                            .iter_mut()
                            .find(|rel| rel.table == fk_info.table && rel.alias == relation_field);

                        if let None = find {
                            related_fields.push(RemainingQuery {
                                fields: vec![remaining],
                                table: fk_info.table,
                                fk: relation_field.to_string(),
                                alias: relation_field.to_string(),
                                fk_on_target: None,
                                //@TODO check for a better way of handling the app_name
                                app_context: AppContext {
                                    app_name: fk_info
                                        .schema
                                        .split_once("010")
                                        .unwrap_or((&fk_info.schema, ""))
                                        .0
                                        .to_string(),
                                    version: context.version.clone(),
                                    request_source: context.request_source.clone(),
                                },
                            });
                        } else {
                            find.unwrap().fields.push(remaining);
                        }

                        stmt.column((Alias::new(table), Alias::new(relation_field)));
                    }
                    // No FK column on the base table — try a virtual 1:M relation.
                    None => {
                        let base_def = base_def.ok_or_else(|| {
                            AppError::NotFound(format!(
                                "Relation {} on collection {} not found",
                                relation_field, table
                            ))
                        })?;

                        let Direction::OneToMany {
                            target_collection,
                            fk_column,
                            base_pk_column,
                            ..
                        } = detect_direction(
                            relation_field,
                            table,
                            base_def,
                            all_collections,
                            None,
                            None,
                        )?
                        else {
                            return Err(AppError::NotFound(format!(
                                "Relation {} on collection {} not found",
                                relation_field, table
                            )));
                        };

                        let find = related_fields.iter_mut().find(|rel| {
                            rel.table == target_collection
                                && rel.alias == relation_field
                                && rel.fk == base_pk_column
                        });

                        if let None = find {
                            related_fields.push(RemainingQuery {
                                fields: vec![remaining],
                                table: target_collection,
                                fk: base_pk_column,
                                alias: relation_field.to_string(),
                                fk_on_target: Some(fk_column),
                                app_context: AppContext {
                                    app_name: context.app_name.clone(),
                                    version: context.version.clone(),
                                    request_source: context.request_source.clone(),
                                },
                            });
                        } else {
                            find.unwrap().fields.push(remaining);
                        }
                        // 1:M values are attached by the follow-up query; there
                        // is no column to select on the base table.
                    }
                }
            } else {
                stmt.column((Alias::new(table), Alias::new(field)));
            }
        }

        // 1:M grouping links on the base PK, so it must be present in the
        // result even when the caller did not ask for it.
        if related_fields.iter().any(|rq| rq.fk_on_target.is_some()) {
            if let Some(pk) = schema
                .columns
                .iter()
                .find(|c| c.table == table && c.is_primary_key)
            {
                if !self.fields.iter().any(|f| f == &pk.name) {
                    stmt.column((Alias::new(table), Alias::new(pk.name.clone())));
                }
            }
        }

        // Now we must also apply the filters which we do trough the add_filter fn
        let filters = self.clone().filter;
        if let Some(and) = filters._and {
            for filter in and {
                let mut condition = Condition::all();
                condition =
                    self.add_filter(schema, context, condition, &filter, &table, &vec![])?;
                stmt.cond_where(condition);
            }
        }

        if let Some(or) = filters._or {
            for filter in or {
                let mut condition = Condition::any();
                condition =
                    self.add_filter(schema, context, condition, &filter, &table, &vec![])?;
                stmt.cond_where(condition);
            }
        }

        for sort in &self.sort {
            let mut order = Order::Asc;
            let field = if sort.starts_with("+") {
                sort.replace("+", "")
            } else if sort.starts_with("-") {
                order = Order::Desc;
                sort.replace("-", "")
            } else {
                sort.to_string()
            };
            stmt.order_by(Alias::new(field), order);
        }

        for join in &self.joins {
            stmt.join_as(
                JoinType::LeftJoin,
                (
                    Alias::new(&join.target_schema),
                    Alias::new(&join.target_table),
                ),
                Alias::new(&join.id),
                Expr::col((Alias::new(&join.id), Alias::new(&join.field)))
                    .equals((Alias::new(&join.source_table), Alias::new(&join.path))),
            );
        }

        Ok((stmt, related_fields))
    }

    /// Creates a condition based on the provided logic
    ///
    /// * `logic` - The LogicOp struct which holds the or/and
    /// * `path` - The path that the query/condition took when generating joins (to prevent duplicate joins)
    fn process_logic(
        &mut self,
        schema: &CoreDatabaseSchema,
        context: &AppContext,
        logic: &LogicOp,
        current_table: &str,
        path: &Vec<&str>,
    ) -> Result<Condition, AppError> {
        if let Some(and) = &logic._and {
            let mut condition = Condition::all();

            for field in and {
                condition =
                    self.add_filter(schema, context, condition, field, current_table, &path)?;
            }

            return Ok(condition);
        }
        if let Some(and) = &logic._or {
            let mut condition = Condition::any();

            for field in and {
                condition =
                    self.add_filter(schema, context, condition, field, current_table, &path)?;
            }

            return Ok(condition);
        }

        Ok(Condition::all())
    }

    /// Devides based on the provided filter if there is logic (_and/_or), or adds the actual filters to the condition
    ///
    /// * `tabconditionle` - The target condition
    /// * `filter` - The filter
    /// * `current_table` - The name of the current table
    /// * `path` - The path that the query/condition took when generating joins (to prevent duplicate joins)
    fn add_filter(
        &mut self,
        schema: &CoreDatabaseSchema,
        context: &AppContext,
        mut condition: Condition,
        filter: &Filter,
        current_table: &str,
        path: &Vec<&str>,
    ) -> Result<Condition, AppError> {
        if let Filter::Logic(logic) = filter {
            condition =
                condition.add(self.process_logic(schema, context, logic, current_table, path)?);
        } else if let Filter::Field(field) = filter {
            condition =
                self.add_field_filter(schema, context, condition, field, current_table, path)?;
        }

        Ok(condition)
    }

    /// Adds the actual filter (eq/neq ect) to the provided condition
    ///
    /// * `tabconditionle` - The target condition
    /// * `field_filter` - The hashmap holding <field:filter_value>
    /// * `path` - The path that the query/condition took when generating joins (to prevent duplicate joins)
    fn add_field_filter(
        &mut self,
        schema: &CoreDatabaseSchema,
        context: &AppContext,
        mut condition: Condition,
        field_filter: &FieldFilter,
        current_table: &str,
        path: &Vec<&str>,
    ) -> Result<Condition, AppError> {
        for (field, value) in &field_filter.fields {
            match value {
                FieldValue::Comparison(comparison) => {
                    let col = Expr::col((
                        Alias::new(current_table.to_string()),
                        Alias::new(&field.to_string()),
                    ));

                    let comparisons: Vec<(
                        &Option<Value>,
                        Box<dyn Fn(Expr, SimpleExpr) -> SimpleExpr>,
                    )> = vec![
                        (&comparison._eq, Box::new(|c, v| c.eq(v))),
                        (&comparison._neq, Box::new(|c, v| c.ne(v))),
                        (&comparison._lt, Box::new(|c, v| c.lt(v))),
                        (&comparison._lte, Box::new(|c, v| c.lte(v))),
                        (&comparison._gt, Box::new(|c, v| c.gt(v))),
                        (&comparison._gte, Box::new(|c, v| c.gte(v))),
                    ];

                    for (opt_val, op) in comparisons {
                        if let Some(val) = opt_val {
                            let parsed =
                                parse_value(schema, &self.joins, current_table, field, val.clone());

                            if let None = parsed {
                                return Err(AppError::BadRequest(format!(
                                    "The column '{}' in invalid or has invalid input",
                                    field
                                )));
                            }
                            let parsed = parsed.unwrap();

                            condition = condition.add(op(col.clone(), parsed));
                        }
                    }
                    let comparisons: Vec<(
                        &Option<(Value, Value)>,
                        Box<dyn Fn(Expr, SimpleExpr, SimpleExpr) -> SimpleExpr>,
                    )> = vec![
                        (
                            &comparison._between,
                            Box::new(|c, v1, v2| c.between(v1, v2)),
                        ),
                        (
                            &comparison._nbetween,
                            Box::new(|c, v1, v2| c.not_between(v1, v2)),
                        ),
                    ];

                    for (opt_val, op) in comparisons {
                        if let Some(val) = opt_val {
                            let parsed1 = parse_value(
                                schema,
                                &self.joins,
                                current_table,
                                field,
                                val.0.clone(),
                            );
                            if let None = parsed1 {
                                return Err(AppError::BadRequest(format!(
                                    "The column '{}' in invalid or has invalid input",
                                    field
                                )));
                            }
                            let parsed2 = parse_value(
                                schema,
                                &self.joins,
                                current_table,
                                field,
                                val.1.clone(),
                            );
                            let parsed1 = parsed1.unwrap();
                            let parsed2 = parsed2.unwrap();

                            condition = condition.add(op(col.clone(), parsed1, parsed2));
                        }
                    }

                    if let Some(_in) = &comparison._in {
                        let in_values = self.value_to_vec(_in, schema, current_table, field)?;

                        condition = condition.add(col.clone().is_in(in_values));
                    }

                    if let Some(_nin) = &comparison._nin {
                        condition = condition.add(col.clone().is_not_in(self.value_to_vec(
                            _nin,
                            schema,
                            current_table,
                            field,
                        )?));
                    }

                    if let Some(_null) = &comparison._null {
                        if _null == &true {
                            condition = condition.add(col.clone().is_null());
                        }
                    }

                    if let Some(_nnull) = &comparison._nnull {
                        if _nnull == &true {
                            condition = condition.add(col.clone().is_not_null());
                        }
                    }

                    if let Some(_contains) = &comparison._contains {
                        let escaped = _contains.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().like(format!("%{}%", escaped)));
                    }

                    if let Some(_ncontains) = &comparison._ncontains {
                        let escaped = _ncontains.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().not_like(format!("%{}%", escaped)));
                    }

                    if let Some(_icontains) = &comparison._icontains {
                        let escaped = _icontains.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().ilike(format!("%{}%", escaped)));
                    }

                    if let Some(_nicontains) = &comparison._nicontains {
                        let escaped = _nicontains.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().not_ilike(format!("%{}%", escaped)));
                    }
                    if let Some(_starts_with) = &comparison._starts_with {
                        let escaped = _starts_with.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().like(format!("{}%", escaped)));
                    }
                    if let Some(_istarts_with) = &comparison._istarts_with {
                        let escaped = _istarts_with.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().ilike(format!("{}%", escaped)));
                    }
                    if let Some(_nstarts_with) = &comparison._nstarts_with {
                        let escaped = _nstarts_with.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().not_like(format!("{}%", escaped)));
                    }
                    if let Some(_nistarts_with) = &comparison._nistarts_with {
                        let escaped = _nistarts_with.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().not_ilike(format!("{}%", escaped)));
                    }
                    if let Some(_ends_with) = &comparison._ends_with {
                        let escaped = _ends_with.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().like(format!("%{}", escaped)));
                    }
                    if let Some(_iends_with) = &comparison._iends_with {
                        let escaped = _iends_with.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().ilike(format!("%{}", escaped)));
                    }
                    if let Some(_nends_with) = &comparison._nends_with {
                        let escaped = _nends_with.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().not_like(format!("%{}", escaped)));
                    }
                    if let Some(_niends_with) = &comparison._niends_with {
                        let escaped = _niends_with.replace('%', r"\%").replace('_', r"\_");

                        condition = condition.add(col.clone().not_ilike(format!("%{}", escaped)));
                    }
                }
                FieldValue::Nested(nested_filter) => {
                    let mut current_table = current_table.to_string();
                    let mut path = path.clone();
                    path.push(field);

                    let path_str = path.join(":");
                    let find_schema = self
                        .joins
                        .iter()
                        .find(|f| f.source_table == current_table && f.path == path_str);

                    match find_schema {
                        Some(val) => current_table = val.id.clone(),
                        None => {
                            let id: String = Uuid::new_v4().to_string();

                            let target_table = schema.clone();
                            let target_table = target_table.columns.iter().find(|col| {
                                col.schema == context.schema_name()
                                    && col.table == current_table
                                    && col.name == *field
                            });

                            if let Some(t) = target_table {
                                if let Some(fk) = &t.foreign_key {
                                    self.joins.push(TableJoin {
                                        id: id.clone(),
                                        path: path_str,
                                        field: fk.column.clone(),
                                        target_table: fk.table.clone(),
                                        source_table: current_table.to_string(),
                                        target_schema: fk.schema.clone(),
                                    });
                                    current_table = id;
                                }
                            }
                        }
                    }

                    condition = self.add_field_filter(
                        schema,
                        context,
                        condition,
                        nested_filter,
                        &current_table,
                        &path,
                    )?;
                }
            }
        }
        Ok(condition)
    }

    /// Converts the value into a Vec<SimpleExpr>
    ///
    /// * `value` - The value
    /// * `parse_schema` - A reference to the DatabaseSchema
    /// * `current_table` - The target table
    /// * `field` - The field name
    fn value_to_vec(
        &mut self,
        value: &serde_json::Value,
        parse_schema: &CoreDatabaseSchema,
        current_table: &str,
        field: &str,
    ) -> Result<Vec<SimpleExpr>, AppError> {
        let values: Vec<SimpleExpr> = if value.is_array() {
            value
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|v| {
                    parse_value(parse_schema, &self.joins, current_table, field, v.clone()).unwrap()
                })
                .collect()
        } else {
            let parsed = parse_value(
                parse_schema,
                &self.joins,
                current_table,
                field,
                value.clone(),
            );
            if let None = parsed {
                return Err(AppError::BadRequest(format!(
                    "The column '{}' in invalid or has invalid input",
                    field
                )));
            }
            let parsed = parsed.unwrap();
            vec![parsed]
        };

        return Ok(values);
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct TableJoin {
    pub id: String,
    pub path: String,
    pub field: String,
    pub target_table: String,
    pub source_table: String,
    pub target_schema: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum Filter {
    Field(FieldFilter),
    Logic(LogicOp),
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LogicOp {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _and: Option<Vec<Filter>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _or: Option<Vec<Filter>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FieldFilter {
    #[serde(flatten)]
    pub fields: std::collections::HashMap<String, FieldValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum FieldValue {
    Nested(FieldFilter),
    Comparison(Comparison),
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, ToSchema)]
pub struct Comparison {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _eq: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _neq: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _lt: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _lte: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _gt: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _gte: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _in: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _nin: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _null: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _nnull: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _contains: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _ncontains: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _icontains: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _nicontains: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _starts_with: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _istarts_with: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _nstarts_with: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _nistarts_with: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _ends_with: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _iends_with: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _nends_with: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _niends_with: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _between: Option<(Value, Value)>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _nbetween: Option<(Value, Value)>, //@TODO _intersects,_nintersects,_intersects_bbox,_nintersects_bbox,_regex,_some,_none
}
