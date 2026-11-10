use rand::{Rng, distr::Alphanumeric};
use sea_query::{
    Alias, Condition, Expr, JoinType, Order, PostgresQueryBuilder, SelectStatement, SimpleExpr,
    extension::postgres::PgExpr,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::str::FromStr;
use std::vec;
use utoipa::ToSchema;

use super::filter_relations::{HopMap, OneToManyHop, resolve_filter_relation_hops};
use crate::services::context::AppContext;
use crate::services::permissions::read::{ReadAccess, ReadRule, resolve_read_access};
use crate::services::postgres::jsonvalue_simpleexpr::parse_value;
use crate::services::postgres::pool::pgrow_to_json;
use crate::{
    AppState,
    services::{
        errors::AlcedoError,
        postgres::{inspector::DatabaseSchema, pool::execute_query},
    },
};

const LIMIT_DEFAULT: u64 = 200;

fn limit_default() -> u64 {
    LIMIT_DEFAULT
}

/// Indices of the rules that grant access to `field`: a rule grants it when its
/// whitelist is absent (`None` = every field) or explicitly contains `field`.
fn field_rule_indices(rules: &[ReadRule], field: &str) -> Vec<usize> {
    rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| match &rule.fields {
            None => true,
            Some(fields) => fields.iter().any(|f| f == field),
        })
        .map(|(index, _)| index)
        .collect()
}

/// True when any rule grants every field with no row conditions, meaning the
/// caller's access degenerates to "all fields, all rows".
fn has_unrestricted_rule(rules: &[ReadRule]) -> bool {
    rules
        .iter()
        .any(|rule| rule.fields.is_none() && rule.conditions.is_empty())
}

/// Whitelisted aggregate functions for `aggregate[fn]=field`.
const AGGREGATE_FNS: &[&str] = &[
    "count",
    "countAll",
    "countDistinct",
    "sum",
    "sumDistinct",
    "avg",
    "avgDistinct",
    "min",
    "max",
];

/// Whitelisted date-part functions for `groupBy[]=fn(field)`.
const GROUP_FNS: &[&str] = &[
    "year", "month", "day", "hour", "minute", "second", "quarter", "week", "dow", "doy", "date",
];

/// Quotes a column identifier for raw SQL fragments. Callers must have
/// validated the name against `DatabaseSchema.columns` first.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', ""))
}

/// Parses one `groupBy` entry into `(field, sql_expr, alias)`. Plain fields
/// select the column itself; `fn(field)` wraps it in `EXTRACT`/`DATE_TRUNC`.
fn parse_group_expr(entry: &str) -> Result<(String, String, String), AlcedoError> {
    let entry = entry.trim();
    if let Some(open) = entry.find('(') {
        if open == 0 || !entry.ends_with(')') {
            return Err(AlcedoError::InvalidInput(
                format!("Invalid groupBy expression '{entry}'"),
                0,
            ));
        }
        let fn_name = &entry[..open];
        let field = entry[open + 1..entry.len() - 1].trim().to_string();
        if !GROUP_FNS.contains(&fn_name) {
            return Err(AlcedoError::InvalidInput(
                format!("Unknown groupBy function '{fn_name}'"),
                0,
            ));
        }
        if field.is_empty() || field == "*" {
            return Err(AlcedoError::InvalidInput(
                format!("Invalid groupBy expression '{entry}'"),
                0,
            ));
        }
        let col = quote_ident(&field);
        // EXTRACT returns NUMERIC, which the row decoder cannot map to JSON;
        // cast to a supported type (`second` can be fractional).
        let sql = if fn_name == "date" {
            format!("DATE_TRUNC('day', {col})")
        } else if fn_name == "second" {
            format!("EXTRACT(SECOND FROM {col})::float8")
        } else {
            format!("EXTRACT({} FROM {col})::int8", fn_name.to_uppercase())
        };
        Ok((field.clone(), sql, format!("{fn_name}_{field}")))
    } else {
        if entry.is_empty() || entry == "*" {
            return Err(AlcedoError::InvalidInput(
                format!("Invalid groupBy expression '{entry}'"),
                0,
            ));
        }
        Ok((entry.to_string(), quote_ident(entry), entry.to_string()))
    }
}

/// A list of fields that accepts either a single (possibly comma-joined)
/// string — `aggregate[count]=a,b` — or a sequence — `aggregate[count][]=a`.
/// serde_qs hands the bare form to the value deserializer as a plain string.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FieldList(pub Vec<String>);

impl<'de> Deserialize<'de> for FieldList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrVec {
            One(String),
            Many(Vec<String>),
        }
        Ok(match StringOrVec::deserialize(deserializer)? {
            StringOrVec::One(s) => FieldList(vec![s]),
            StringOrVec::Many(v) => FieldList(v),
        })
    }
}

/// Parses one `aggregate[fn]=field` pair into `(field, sql_expr, alias)`.
/// `*` is only meaningful for `count`/`countAll` and maps to `COUNT(*)`.
fn parse_aggregate_expr(
    fn_name: &str,
    field: &str,
) -> Result<(String, String, String), AlcedoError> {
    if !AGGREGATE_FNS.contains(&fn_name) {
        return Err(AlcedoError::InvalidInput(
            format!("Unknown aggregate function '{fn_name}'"),
            0,
        ));
    }
    if field == "*" && !matches!(fn_name, "count" | "countAll") {
        return Err(AlcedoError::InvalidInput(
            format!("Aggregate function '{fn_name}' does not support '*'"),
            0,
        ));
    }
    let alias = format!("{fn_name}_{field}");
    // SUM/AVG over numeric columns return NUMERIC, which the row decoder
    // cannot map to JSON; cast to float8 (COUNT returns INT8 already).
    let sql = match fn_name {
        "countAll" => "COUNT(*)".to_string(),
        "count" if field == "*" => "COUNT(*)".to_string(),
        "count" => format!("COUNT({})", quote_ident(field)),
        "countDistinct" => format!("COUNT(DISTINCT {})", quote_ident(field)),
        "sum" => format!("SUM({})::float8", quote_ident(field)),
        "sumDistinct" => format!("SUM(DISTINCT {})::float8", quote_ident(field)),
        "avg" => format!("AVG({})::float8", quote_ident(field)),
        "avgDistinct" => format!("AVG(DISTINCT {})::float8", quote_ident(field)),
        "min" => format!("MIN({})", quote_ident(field)),
        "max" => format!("MAX({})", quote_ident(field)),
        _ => unreachable!("fn whitelist checked above"),
    };
    Ok((field.to_string(), sql, alias))
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
    /// Number of rows to skip. UI also sends `page`/`per_page`, which
    /// `normalize_pagination` converts into `limit`/`offset`.
    #[serde(default)]
    pub offset: u64,
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub per_page: Option<u64>,
    #[serde(default, rename = "includeCount")]
    pub include_count: bool,
    /// Directus-style aggregation: `aggregate[fn]=field[,field...]`.
    /// serde_qs does not split comma-joined values, so `to_sql` normalizes
    /// them into one entry per field via `normalize_aggregate`.
    #[serde(default)]
    pub aggregate: HashMap<String, FieldList>,
    /// Directus-style grouping: `groupBy[]=field` or `groupBy[]=fn(field)`.
    #[serde(default, rename = "groupBy")]
    pub group_by: Vec<String>,
    /// Virtual 1:M relation hops referenced by `filter`, resolved just before
    /// the SQL is built. Not part of the wire format.
    #[serde(skip)]
    pub relation_hops: HopMap,
    /// Record-level read access resolved for the current caller. Not part of
    /// the wire format; injected into `filter` by `apply_access`.
    #[serde(skip)]
    pub access: ReadAccess,
    /// Guards `apply_access` so the access filter is injected at most once.
    #[serde(skip)]
    pub access_injected: bool,
}

impl Query {
    /// Translates `page` + `per_page` into `limit` / `offset`. A direct `limit`/`offset` still wins if `per_page` is absent.
    pub fn normalize_pagination(&mut self) {
        if let Some(per_page) = self.per_page {
            if per_page > 0 {
                let page = self.page.unwrap_or(1).max(1);
                self.limit = per_page;
                self.offset = (page - 1) * per_page;
            }
        }
    }

    /// Splits comma-joined aggregate values (`aggregate[count]=a,b`) into
    /// one entry per field. serde_qs does not split commas itself.
    pub fn normalize_aggregate(&mut self) {
        if self.aggregate.is_empty() {
            return;
        }
        let mut normalized: HashMap<String, FieldList> = HashMap::new();
        for (fn_name, FieldList(values)) in std::mem::take(&mut self.aggregate) {
            let mut fields = Vec::new();
            for value in values {
                for part in value.split(',') {
                    let part = part.trim();
                    if !part.is_empty() {
                        fields.push(part.to_string());
                    }
                }
            }
            normalized.insert(fn_name, FieldList(fields));
        }
        self.aggregate = normalized;
    }

    /// Validates a group/aggregate field against the table's columns.
    /// `*` is allowed (only `count`/`countAll` accept it, checked by the
    /// parser); dotted relation fields are rejected.
    fn validate_agg_field(
        &self,
        table: &str,
        schema: &DatabaseSchema,
        context: &AppContext,
        field: &str,
    ) -> Result<(), AlcedoError> {
        if field == "*" {
            return Ok(());
        }
        if field.is_empty() || field.contains('.') {
            return Err(AlcedoError::InvalidInput(
                format!("Invalid field '{field}' in aggregate/groupBy"),
                0,
            ));
        }
        let exists = schema
            .columns
            .iter()
            .any(|c| c.table == table && c.schema == context.schema_name() && c.name == field);
        if !exists {
            return Err(AlcedoError::InvalidInput(
                format!("Field '{field}' does not exist on collection '{table}'"),
                0,
            ));
        }
        Ok(())
    }

    /// Under `Restricted` access, every aggregated/grouped field must be
    /// granted by some rule; `*` additionally requires an unrestricted rule.
    fn validate_agg_access(&self, field: &str) -> Result<(), AlcedoError> {
        if let ReadAccess::Restricted { rules } = &self.access {
            if field == "*" {
                if !has_unrestricted_rule(rules) {
                    return Err(AlcedoError::Forbidden(
                        "No access to aggregate over all fields".to_string(),
                        0,
                    ));
                }
            } else if !has_unrestricted_rule(rules) && field_rule_indices(rules, field).is_empty() {
                return Err(AlcedoError::Forbidden(
                    format!("No access to field '{field}'"),
                    0,
                ));
            }
        }
        Ok(())
    }

    /// Builds the group/aggregate select expressions for `to_sql`, returning
    /// `(group_exprs, aggregate_exprs)` as `(sql, alias)` pairs. Aggregate
    /// exprs are sorted by alias so the generated SQL is deterministic.
    fn build_aggregation(
        &mut self,
        table: &str,
        schema: &DatabaseSchema,
        context: &AppContext,
    ) -> Result<(Vec<(String, String)>, Vec<(String, String)>), AlcedoError> {
        self.normalize_aggregate();

        let mut group_exprs: Vec<(String, String)> = Vec::new();
        for entry in &self.group_by {
            let (field, sql, alias) = parse_group_expr(entry)?;
            self.validate_agg_field(table, schema, context, &field)?;
            self.validate_agg_access(&field)?;
            group_exprs.push((sql, alias));
        }

        let mut agg_exprs: Vec<(String, String)> = Vec::new();
        for (fn_name, FieldList(fields)) in &self.aggregate {
            for field in fields {
                let (field, sql, alias) = parse_aggregate_expr(fn_name, field)?;
                self.validate_agg_field(table, schema, context, &field)?;
                self.validate_agg_access(&field)?;
                agg_exprs.push((sql, alias));
            }
        }
        agg_exprs.sort_by(|a, b| a.1.cmp(&b.1));

        Ok((group_exprs, agg_exprs))
    }

    /// Injects the resolved record-level read access into `filter._and`.
    ///
    /// Each rule's conditions are AND-ed together, the rules are OR-ed among
    /// themselves, and the whole permission clause is AND-ed with the caller's
    /// filter. Idempotent: repeated calls are no-ops.
    pub fn apply_access(&mut self) {
        if self.access_injected {
            return;
        }
        self.access_injected = true;

        if matches!(self.access, ReadAccess::Unrestricted | ReadAccess::Deny) {
            return;
        }

        if let ReadAccess::Restricted { rules } = &self.access {
            if rules.is_empty() {
                self.access = ReadAccess::Deny;
                return;
            }

            let rule_filters: Vec<Filter> = rules
                .iter()
                .map(|rule| {
                    Filter::Logic(LogicOp {
                        _and: Some(rule.conditions.clone()),
                        _or: None,
                    })
                })
                .collect();

            let access_filter = Filter::Logic(LogicOp {
                _or: Some(rule_filters),
                _and: None,
            });

            self.filter
                ._and
                .get_or_insert_with(Vec::new)
                .push(access_filter);
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemainingQuery {
    pub table: String,
    pub schema: String,
    pub fields: Vec<String>,
    pub fk: String,
    pub app_context: AppContext,
}

impl Query {
    /// A query filtered on a single equality (`field = value`).
    pub fn eq(field: &str, value: Value) -> Self {
        Query::eq_all(&[(field, value)])
    }

    /// A query filtered on several equalities, combined with `AND`.
    pub fn eq_all(fields: &[(&str, Value)]) -> Self {
        let mut map: HashMap<String, FieldValue> = HashMap::new();
        for (field, value) in fields {
            map.insert(
                (*field).to_string(),
                FieldValue::Comparison(Comparison::eq(value.clone())),
            );
        }
        Query {
            filter: LogicOp {
                _and: Some(vec![Filter::Field(FieldFilter { fields: map })]),
                _or: None,
            },
            ..Default::default()
        }
    }

    /// Executes the Query object as a sql query on the database
    ///
    /// * `state` - The appstate
    /// * `table` - The target table
    pub async fn execute_query(
        &mut self,
        context: &AppContext,
        state: &AppState,
        table: &String,
    ) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        self.apply_access();
        if matches!(self.access, ReadAccess::Deny) {
            return Ok(Vec::new());
        }

        self.relation_hops = Box::pin(resolve_filter_relation_hops(
            state,
            context,
            table,
            &self.filter,
        ))
        .await?;

        let schema = state.database_schema.read().await;
        let (stmt, remaining_queries) = self.to_sql(&table, &schema, context)?;
        drop(schema);
        // Because we can have relations (for example fields[]=customer.name) we will have a query that will need to execute after this one aka "remaining"
        let query_str = stmt.to_string(PostgresQueryBuilder);

        let result = execute_query(&state, query_str).await?;
        let mut rows: Vec<Map<String, Value>> = result
            .into_iter()
            .filter_map(|row| pgrow_to_json(&row).ok())
            .collect();

        let schema = state.database_schema.read().await;

        // Lets execute the remaining queries based on the fk`s that came from our "base" query
        for rq in remaining_queries {
            let fk_vals: Vec<String> = rows
                .iter()
                .filter_map(|row| row.get(&rq.fk))
                .filter(|v| !v.is_null())
                .map(|v| v.to_string())
                .collect();

            if fk_vals.is_empty() {
                continue;
            }

            // Enforce the caller's read rules on the related collection. A
            // `Deny` drops the relation entirely (NULL) so the raw fk value
            // does not leak; any restriction also lets us NULL rows whose
            // related record was filtered out by policy.
            let access = Box::pin(resolve_read_access(
                state,
                &rq.app_context,
                &rq.table,
                rq.app_context.identity.as_ref(),
            ))
            .await?;

            if matches!(access, ReadAccess::Deny) {
                for row in rows.iter_mut() {
                    row.insert(rq.fk.clone(), Value::Null);
                }
                continue;
            }
            let nested_restricted = !matches!(access, ReadAccess::Unrestricted);

            let pk = match schema
                .columns
                .iter()
                .find(|c| c.table == rq.table && c.is_primary_key)
            {
                Some(v) => v,
                None => continue,
            };

            // lets create a new Query object to fetch the relations
            // we do this by a simple where fk in (id1,id2 ect)
            let mut field_filter = HashMap::new();

            field_filter.insert(
                pk.name.clone(),
                FieldValue::Comparison(Comparison {
                    _in: Some(
                        fk_vals
                            .iter()
                            .filter_map(|s| Value::from_str(s).ok())
                            .collect(),
                    ),
                    ..Default::default()
                }),
            );

            let filter = LogicOp {
                _and: Some(vec![Filter::Field(FieldFilter {
                    fields: field_filter,
                })]),
                _or: None,
            };

            let mut nested_fields = rq.fields.clone();
            nested_fields.push(pk.name.clone());
            let mut nested_query = Query {
                fields: nested_fields,
                filter,
                limit: 0,
                ..Default::default()
            };
            nested_query.access = access.clone();

            let related_rows =
                Box::pin(nested_query.execute_query(&rq.app_context, &state, &rq.table)).await?;

            // After we got the related rows, we will need to replace the "id" in the rows that we got from our first query
            for row in rows.iter_mut() {
                let fk_val = row.get(&rq.fk);
                if let Some(val) = fk_val {
                    let related_value = related_rows
                        .iter()
                        .find(|row| {
                            row.get(&pk.name).unwrap_or(&Value::String("".to_string())) == val
                        })
                        .map(|v| v.clone());

                    if let Some(val) = related_value {
                        row.insert(rq.fk.clone(), val.into());
                    } else if nested_restricted {
                        // The related record may have been hidden by policy:
                        // drop the raw fk instead of leaking it.
                        row.insert(rq.fk.clone(), Value::Null);
                    }
                }
            }
        }
        drop(schema);
        Ok(rows)
    }

    /// Converts the Query into a sea_query select statement
    ///
    /// * `table` - The target table
    /// * `schema` - The database schema
    pub fn to_sql(
        &mut self,
        table: &str,
        schema: &DatabaseSchema,
        context: &AppContext,
    ) -> Result<(SelectStatement, Vec<RemainingQuery>), AlcedoError> {
        self.apply_access();

        if matches!(self.access, ReadAccess::Deny) {
            return Err(AlcedoError::Forbidden(
                format!("No access to collection '{}'", table),
                0,
            ));
        }
        let table_schema = schema
            .tables
            .iter()
            .find(|t| t.name == table && t.schema == context.schema_name());
        if let None = table_schema {
            return Err(AlcedoError::NotFound(
                format!("Collection '{}' not found", table),
                1,
            ));
        }
        // because theoreticly someone could provide the joins trough the API, we will always overwrite this
        self.joins = vec![];
        let mut stmt = sea_query::Query::select();

        stmt.from((Alias::new(context.schema_name()), Alias::new(table)));

        if self.limit != 0 {
            stmt.limit(self.limit);
        }
        if self.offset != 0 {
            stmt.offset(self.offset);
        }

        // when `aggregate` or `groupBy` is present, the select list is built from group/aggregate
        // expressions instead of plain columns. Filters, sort, limit and
        // offset below still apply, and `count_items_by_query` wrapping this
        // select in COUNT(*) yields the group count for `includeCount`.
        let aggregating = !self.aggregate.is_empty() || !self.group_by.is_empty();
        if aggregating {
            let (group_exprs, agg_exprs) = self.build_aggregation(table, schema, context)?;
            for (expr, alias) in &group_exprs {
                stmt.expr_as(Expr::cust(expr.clone()), Alias::new(alias.clone()));
            }
            for (expr, alias) in &agg_exprs {
                stmt.expr_as(Expr::cust(expr.clone()), Alias::new(alias.clone()));
            }
            stmt.add_group_by(group_exprs.iter().map(|(expr, _)| Expr::cust(expr.clone())));
            self.fields = vec![];
        }

        // if there are no fields give, we assume that all values will need to be returned
        if self.fields.is_empty() && !aggregating {
            let table_fields = schema
                .columns
                .iter()
                .filter(|f| f.table == table && f.schema == context.schema_name())
                .map(|f| format!("{}", f.name))
                .collect();
            self.fields = table_fields;
        } else if !aggregating {
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

        // Record-level field masking is only ever applied for `Restricted`
        // access. Clone the rules out so the `add_filter` calls below can
        // borrow `self` mutably.
        let access_rules: Option<Vec<ReadRule>> = match &self.access {
            ReadAccess::Restricted { rules } => Some(rules.clone()),
            _ => None,
        };

        // Dotted fields are grouped by their top-level relation, preserving
        // first-seen order and de-duplicating subfields, so each relation is
        // selected (and masked) exactly once.
        let mut relation_groups: Vec<(String, Vec<String>)> = vec![];
        for field in fields.iter() {
            let Some((relation, subfield)) = field.split_once('.') else {
                continue;
            };
            match relation_groups
                .iter_mut()
                .find(|(name, _)| name == relation)
            {
                Some((_, subfields)) => {
                    if !subfields.iter().any(|f| f == subfield) {
                        subfields.push(subfield.to_string());
                    }
                }
                None => relation_groups.push((relation.to_string(), vec![subfield.to_string()])),
            }
        }

        // based on the provided fields, we should fill the related_fields vector
        for field in fields {
            // Dotted (relation) fields are handled in the group pass below.
            if field.contains('.') {
                continue;
            }
            self.push_column(
                &mut stmt,
                schema,
                context,
                table,
                field,
                access_rules.as_deref(),
            )?;
        }

        for (relation_field, subfields) in relation_groups {
            // Validate the relation still exists and points somewhere.
            let fk_column = schema.columns.iter().find(|col| {
                col.table == table
                    && col.name == relation_field
                    && col.schema == context.schema_name()
            });

            let Some(fk_column) = fk_column else {
                return Err(AlcedoError::NotFound(
                    format!(
                        "Relation {} on collection {} not found",
                        relation_field, table
                    ),
                    1,
                ));
            };

            let Some(fk_info) = fk_column.foreign_key.clone() else {
                return Err(AlcedoError::NotFound(
                    format!(
                        "Relation {} on collection {} not found",
                        relation_field, table
                    ),
                    1,
                ));
            };

            // Relations get the same per-row masking as scalar fields: rows
            // not matching the granting rules see a NULL fk, and the nested
            // fetch naturally skips them.
            if !self.push_column(
                &mut stmt,
                schema,
                context,
                table,
                &relation_field,
                access_rules.as_deref(),
            )? {
                continue;
            }

            let find = related_fields
                .iter_mut()
                .find(|rel| rel.table == fk_info.table && rel.schema == fk_info.schema);

            if let Some(existing) = find {
                for subfield in subfields {
                    if !existing.fields.iter().any(|f| f == &subfield) {
                        existing.fields.push(subfield);
                    }
                }
            } else {
                let app_context = if fk_info.schema == "alcedocore" {
                    AppContext::system(context.request_source.clone())
                } else {
                    let (app_name, version) = fk_info
                        .schema
                        .split_once("010")
                        .map(|(a, v)| (a.to_string(), v.to_string()))
                        .unwrap_or_else(|| (fk_info.schema.clone(), context.version.clone()));
                    AppContext {
                        app_name,
                        version,
                        request_source: context.request_source.clone(),
                        identity: None,
                        request_id: context.request_id.clone(),
                    }
                };
                let mut app_context = app_context;
                app_context.identity = context.identity.clone();
                related_fields.push(RemainingQuery {
                    fields: subfields,
                    table: fk_info.table.clone(),
                    schema: fk_info.schema.clone(),
                    fk: relation_field,
                    app_context,
                });
            }
        }

        // Now we must also apply the filters which we do trough the add_filter fn
        let filters = self.clone().filter;
        if let Some(and) = filters._and {
            for filter in and {
                let mut condition = Condition::all();
                condition = self.add_filter(
                    schema,
                    context,
                    condition,
                    &filter,
                    &context.schema_name(),
                    &table,
                    &vec![],
                )?;
                stmt.cond_where(condition);
            }
        }

        if let Some(or) = filters._or {
            let mut any = Condition::any();
            for filter in or {
                let mut condition = Condition::all();
                condition = self.add_filter(
                    schema,
                    context,
                    condition,
                    &filter,
                    &context.schema_name(),
                    &table,
                    &vec![],
                )?;
                any = any.add(condition);
            }
            stmt.cond_where(any);
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
                Expr::col((Alias::new(&join.id), Alias::new(&join.field))).equals((
                    Alias::new(&join.source_table),
                    Alias::new(&join.source_column),
                )),
            );
        }

        Ok((stmt, related_fields))
    }

    /// Appends `field` from `table` to `stmt`, applying record-level per-row
    /// masking when `rules` is `Some` (i.e. the collection is `Restricted`).
    ///
    /// Returns whether the column was emitted: a field not granted by any rule
    /// is omitted entirely. `id`/`created_at`/`updated_at`/the primary key are
    /// always readable, and a rule granting every field with no conditions
    /// degenerates to unmasked.
    fn push_column(
        &mut self,
        stmt: &mut SelectStatement,
        schema: &DatabaseSchema,
        context: &AppContext,
        table: &str,
        field: &str,
        rules: Option<&[ReadRule]>,
    ) -> Result<bool, AlcedoError> {
        let Some(rules) = rules else {
            stmt.column((Alias::new(table), Alias::new(field)));
            return Ok(true);
        };

        let pk_name: Option<String> = schema
            .columns
            .iter()
            .find(|c| c.table == table && c.schema == context.schema_name() && c.is_primary_key)
            .map(|c| c.name.clone());
        let always_plain = field == "id"
            || field == "created_at"
            || field == "updated_at"
            || pk_name.as_deref() == Some(field);

        if always_plain || has_unrestricted_rule(rules) {
            stmt.column((Alias::new(table), Alias::new(field)));
            return Ok(true);
        }

        let indices = field_rule_indices(rules, field);
        if indices.is_empty() {
            // Not granted by any rule: omit the field entirely.
            return Ok(false);
        }
        if indices.len() == rules.len() {
            stmt.column((Alias::new(table), Alias::new(field)));
            return Ok(true);
        }

        // Granted by some (but not all) rules: mask each row with a CASE that
        // reads the column only when one of the granting rules matches.
        let allowed_rules: Vec<Filter> = indices
            .iter()
            .map(|&index| {
                Filter::Logic(LogicOp {
                    _and: Some(rules[index].conditions.clone()),
                    _or: None,
                })
            })
            .collect();
        let allowed_filter = Filter::Logic(LogicOp {
            _or: Some(allowed_rules),
            _and: None,
        });
        let condition = self.add_filter(
            schema,
            context,
            Condition::all(),
            &allowed_filter,
            &context.schema_name(),
            table,
            &vec![],
        )?;
        stmt.expr_as(
            Expr::case(condition, Expr::col((Alias::new(table), Alias::new(field)))),
            Alias::new(field),
        );
        Ok(true)
    }

    /// Creates a condition based on the provided logic
    ///
    /// * `logic` - The LogicOp struct which holds the or/and
    /// * `path` - The path that the query/condition took when generating joins (to prevent duplicate joins)
    fn process_logic(
        &mut self,
        schema: &DatabaseSchema,
        context: &AppContext,
        logic: &LogicOp,
        current_schema: &str,
        current_table: &str,
        path: &Vec<&str>,
    ) -> Result<Condition, AlcedoError> {
        if let Some(and) = &logic._and {
            let mut condition = Condition::all();

            for field in and {
                condition = self.add_filter(
                    schema,
                    context,
                    condition,
                    field,
                    current_schema,
                    current_table,
                    &path,
                )?;
            }

            return Ok(condition);
        }
        if let Some(and) = &logic._or {
            let mut condition = Condition::any();

            for field in and {
                condition = self.add_filter(
                    schema,
                    context,
                    condition,
                    field,
                    current_schema,
                    current_table,
                    &path,
                )?;
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
        schema: &DatabaseSchema,
        context: &AppContext,
        mut condition: Condition,
        filter: &Filter,
        current_schema: &str,
        current_table: &str,
        path: &Vec<&str>,
    ) -> Result<Condition, AlcedoError> {
        if let Filter::Logic(logic) = filter {
            condition = condition.add(self.process_logic(
                schema,
                context,
                logic,
                current_schema,
                current_table,
                path,
            )?);
        } else if let Filter::Field(field) = filter {
            condition = self.add_field_filter(
                schema,
                context,
                condition,
                field,
                current_schema,
                current_table,
                path,
            )?;
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
        schema: &DatabaseSchema,
        context: &AppContext,
        mut condition: Condition,
        field_filter: &FieldFilter,
        current_schema: &str,
        current_table: &str,
        path: &Vec<&str>,
    ) -> Result<Condition, AlcedoError> {
        for (field, value) in &field_filter.fields {
            match value {
                FieldValue::Comparison(comparison) => {
                    let col = Expr::col((
                        Alias::new(current_table.to_string()),
                        Alias::new(&field.to_string()),
                    ));

                    let comparisons: Vec<(
                        &Option<_>,
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
                            let parsed = parse_value(
                                schema,
                                current_schema,
                                &self.joins,
                                current_table,
                                field,
                                val.clone(),
                            );

                            if let None = parsed {
                                return Err(AlcedoError::InvalidInput(
                                    format!(
                                        "The column '{}' in invalid or has invalid input",
                                        field
                                    ),
                                    1,
                                ));
                            }
                            let parsed = parsed.unwrap();

                            condition = condition.add(op(col.clone(), parsed));
                        }
                    }
                    let comparisons: Vec<(
                        &Option<_>,
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
                                current_schema,
                                &self.joins,
                                current_table,
                                field,
                                val.0.clone(),
                            );
                            if let None = parsed1 {
                                return Err(AlcedoError::InvalidInput(
                                    format!(
                                        "The column '{}' in invalid or has invalid input",
                                        field
                                    ),
                                    1,
                                ));
                            }
                            let parsed2 = parse_value(
                                schema,
                                current_schema,
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
                        let in_values =
                            self.value_to_vec(_in, schema, current_schema, current_table, field)?;

                        condition = condition.add(col.clone().is_in(in_values));
                    }

                    if let Some(_nin) = &comparison._nin {
                        condition = condition.add(col.clone().is_not_in(self.value_to_vec(
                            _nin,
                            schema,
                            current_schema,
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
                    let mut current_schema = current_schema.to_string();
                    let mut path = path.clone();
                    path.push(field);

                    let path_str = path.join(":");
                    let find_join = self
                        .joins
                        .iter()
                        .find(|f| f.source_table == current_table && f.path == path_str);

                    match find_join {
                        Some(val) => {
                            current_schema = val.target_schema.clone();
                            current_table = val.id.clone();
                        }
                        None => {
                            let lookup_table = self
                                .joins
                                .iter()
                                .find(|j| j.id == current_table)
                                .map(|j| j.target_table.clone())
                                .unwrap_or_else(|| current_table.clone());

                            let target_column = schema.columns.iter().find(|col| {
                                col.schema == current_schema
                                    && col.table == lookup_table
                                    && col.name == *field
                            });

                            let mut joined = false;
                            if let Some(t) = target_column {
                                if let Some(fk) = &t.foreign_key {
                                    let id: String = rand::rng()
                                        .sample_iter(&Alphanumeric)
                                        .take(7)
                                        .map(char::from)
                                        .collect();
                                    self.joins.push(TableJoin {
                                        id: id.clone(),
                                        path: path_str,
                                        field: fk.column.clone(),
                                        source_column: field.to_string(),
                                        target_table: fk.table.clone(),
                                        source_table: current_table.to_string(),
                                        target_schema: fk.schema.clone(),
                                    });
                                    current_schema = fk.schema.clone();
                                    current_table = id;
                                    joined = true;
                                }
                            }

                            if !joined {
                                let hop_key = (
                                    current_schema.clone(),
                                    lookup_table.clone(),
                                    field.to_string(),
                                );
                                if let Some(hop) = self.relation_hops.get(&hop_key).cloned() {
                                    condition = self.add_exists_condition(
                                        condition,
                                        schema,
                                        context,
                                        nested_filter,
                                        &current_table,
                                        &lookup_table,
                                        &current_schema,
                                        &hop,
                                        &path,
                                    )?;
                                    continue;
                                }
                            }
                        }
                    }

                    condition = self.add_field_filter(
                        schema,
                        context,
                        condition,
                        nested_filter,
                        &current_schema,
                        &current_table,
                        &path,
                    )?;
                }
            }
        }
        Ok(condition)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_exists_condition(
        &mut self,
        condition: Condition,
        schema: &DatabaseSchema,
        context: &AppContext,
        nested_filter: &FieldFilter,
        parent_alias: &str,
        parent_table: &str,
        parent_schema: &str,
        hop: &OneToManyHop,
        path: &Vec<&str>,
    ) -> Result<Condition, AlcedoError> {
        // Build the child condition in a fresh join scope so the outer query's
        // joins are not polluted.
        let outer_joins = std::mem::take(&mut self.joins);
        let child_condition = self.add_field_filter(
            schema,
            context,
            Condition::all(),
            nested_filter,
            &hop.child_schema,
            &hop.child_table,
            path,
        )?;
        let child_joins = std::mem::take(&mut self.joins);
        self.joins = outer_joins;

        let parent_pk = schema
            .columns
            .iter()
            .find(|c| c.schema == parent_schema && c.table == parent_table && c.is_primary_key)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "id".to_string());

        let mut sub = sea_query::Query::select();
        // EXISTS needs a non-empty target list: PostgreSQL's `SELECT FROM t`
        // yields zero rows, which would make every 1:M condition false.
        sub.expr(Expr::value(1));
        sub.from((Alias::new(&hop.child_schema), Alias::new(&hop.child_table)));
        for join in &child_joins {
            sub.join_as(
                JoinType::LeftJoin,
                (
                    Alias::new(&join.target_schema),
                    Alias::new(&join.target_table),
                ),
                Alias::new(&join.id),
                Expr::col((Alias::new(&join.id), Alias::new(&join.field))).equals((
                    Alias::new(&join.source_table),
                    Alias::new(&join.source_column),
                )),
            );
        }
        sub.cond_where(child_condition);
        sub.and_where(
            Expr::col((Alias::new(&hop.child_table), Alias::new(&hop.fk_column)))
                .equals((Alias::new(parent_alias), Alias::new(&parent_pk))),
        );

        Ok(condition.add(Expr::exists(sub)))
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
        parse_schema: &DatabaseSchema,
        schema_name: &str,
        current_table: &str,
        field: &str,
    ) -> Result<Vec<SimpleExpr>, AlcedoError> {
        let values: Vec<SimpleExpr> = if value.is_array() {
            let mut values = Vec::new();
            for v in value.as_array().unwrap_or(&Vec::new()) {
                match parse_value(
                    parse_schema,
                    schema_name,
                    &self.joins,
                    current_table,
                    field,
                    v.clone(),
                ) {
                    Some(parsed) => values.push(parsed),
                    None => {
                        return Err(AlcedoError::InvalidInput(
                            format!("The column '{}' in invalid or has invalid input", field),
                            1,
                        ));
                    }
                }
            }
            values
        } else {
            let parsed = parse_value(
                parse_schema,
                schema_name,
                &self.joins,
                current_table,
                field,
                value.clone(),
            );
            if let None = parsed {
                return Err(AlcedoError::InvalidInput(
                    format!("The column '{}' in invalid or has invalid input", field),
                    1,
                ));
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
    pub source_column: String,
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

/// Accepts a boolean, but treats JSON `null` as `true`. The admin UI's
/// short-form filter emits `{"field": {"_null": null}}` for "is null".
fn de_nullable_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(Value::Null) => Some(true),
        Some(Value::Bool(b)) => Some(b),
        Some(other) => Some(other.as_bool().unwrap_or(true)),
    })
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

    #[serde(
        default,
        deserialize_with = "de_nullable_bool",
        skip_serializing_if = "Option::is_none"
    )]
    pub _null: Option<bool>,

    #[serde(
        default,
        deserialize_with = "de_nullable_bool",
        skip_serializing_if = "Option::is_none"
    )]
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

impl Comparison {
    /// `field = value`
    pub fn eq(value: Value) -> Self {
        Comparison {
            _eq: Some(value),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(fields: Option<&[&str]>) -> ReadRule {
        ReadRule {
            fields: fields.map(|f| f.iter().map(|s| s.to_string()).collect()),
            conditions: vec![],
            validations: vec![],
            raw_validations: Value::Null,
        }
    }

    #[test]
    fn field_rule_indices_membership_and_wildcard() {
        let rules = vec![rule(Some(&["a", "b"])), rule(Some(&["b"])), rule(None)];
        assert_eq!(field_rule_indices(&rules, "a"), vec![0, 2]);
        assert_eq!(field_rule_indices(&rules, "b"), vec![0, 1, 2]);
        assert_eq!(field_rule_indices(&rules, "c"), vec![2]);
    }

    #[test]
    fn field_rule_indices_empty_rules() {
        assert!(field_rule_indices(&[], "a").is_empty());
    }

    #[test]
    fn unrestricted_rule_detection() {
        assert!(has_unrestricted_rule(&[rule(None)]));

        let mut conditioned = rule(None);
        conditioned
            .conditions
            .push(Filter::Logic(LogicOp::default()));
        assert!(!has_unrestricted_rule(&[conditioned]));

        assert!(!has_unrestricted_rule(&[rule(Some(&["a"]))]));
        assert!(!has_unrestricted_rule(&[]));
    }

    /// A caller-supplied top-level `_or` must not be able to widen the
    /// permission clause: access is injected into `_and` (AND-ed), and the
    /// client's `_or` is left untouched so `to_sql` ANDs it with the rules.
    #[test]
    fn apply_access_is_anded_with_client_supplied_or() {
        let mut query = Query {
            filter: LogicOp {
                _and: None,
                _or: Some(vec![Filter::Logic(LogicOp {
                    _and: Some(
                        conditions_from_json(&serde_json::json!([
                            { "field": "customer", "operator": "eq", "value": "globex" }
                        ]))
                        .unwrap(),
                    ),
                    _or: None,
                })]),
            },
            ..Default::default()
        };
        query.access = ReadAccess::Restricted {
            rules: vec![ReadRule {
                fields: None,
                conditions: conditions_from_json(&serde_json::json!([
                    { "field": "customer", "operator": "eq", "value": "acme" }
                ]))
                .unwrap(),
                validations: vec![],
                raw_validations: Value::Null,
            }],
        };

        query.apply_access();

        let and = query
            .filter
            ._and
            .as_ref()
            .expect("access clause must be injected into _and");
        assert_eq!(and.len(), 1, "exactly one injected access clause");
        assert!(
            matches!(&and[0], Filter::Logic(op) if op._or.is_some() && op._and.is_none()),
            "access clause is an OR of the (single) granting rule"
        );
        assert_eq!(
            query.filter._or.as_ref().map(Vec::len),
            Some(1),
            "client _or must be preserved, never merged with access"
        );
    }

    // ------------------------------------------------------------------
    // DB-backed relation-access tests (seeded `crm010production` app).
    // ------------------------------------------------------------------

    use crate::middelware::auth::AuthLevel;
    use crate::services::context::RequestSource;
    use crate::services::permissions::read::conditions_from_json;
    use crate::services::postgres::inspector::TableMeta;
    use serde_json::json;
    use sqlx::Row as _;

    const CRM_SCHEMA: &str = "crm010production";

    fn crm_ctx(identity: Option<AuthLevel>) -> AppContext {
        AppContext {
            app_name: "crm".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity,
            request_id: None,
        }
    }

    /// System context: trusted, no identity, used by tests that drive `Query`
    /// directly (the access gate is covered separately).
    fn crm_system_ctx() -> AppContext {
        AppContext {
            app_name: "crm".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::Migration,
            identity: None,
            request_id: None,
        }
    }

    async fn inject_customers_meta(state: &AppState) -> Option<i64> {
        let rows = sqlx::query(&format!(
            "SELECT id, app_name, app_version, \"table\", name FROM \"{CRM_SCHEMA}\".alcedocore_collections"
        ))
        .fetch_all(&*state.database_pool)
        .await
        .ok()?;

        let mut customers_id = None;
        let mut schema = state.database_schema.write().await;
        for row in rows {
            let id = row.try_get::<i32, _>("id").ok();
            let Ok(app_name) = row.try_get::<String, _>("app_name") else {
                continue;
            };
            let Ok(app_version) = row.try_get::<String, _>("app_version") else {
                continue;
            };
            let Ok(table) = row.try_get::<String, _>("table") else {
                continue;
            };
            let Ok(name) = row.try_get::<String, _>("name") else {
                continue;
            };
            if table == "customers" {
                customers_id = id.map(i64::from);
            }
            let Some(target) = schema
                .tables
                .iter_mut()
                .find(|t| t.schema == CRM_SCHEMA && t.name == table)
            else {
                continue;
            };
            target.meta = Some(TableMeta {
                id: id.map(i64::from),
                app_name,
                app_version,
                table,
                name,
                icon_name: None,
                icon_color: None,
                singleton: false,
                hidden: false,
                sort_field: None,
            });
        }
        customers_id
    }

    async fn count_contacts(state: &AppState, predicate: &str) -> Option<i64> {
        sqlx::query_scalar::<_, i64>(&format!(
            "SELECT COUNT(*)::bigint FROM \"{CRM_SCHEMA}\".contacts WHERE {predicate}"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .ok()
    }

    /// Test A: a relation granted by a subset of the rules is masked per row
    /// (`CASE WHEN <granting rule> THEN "customer" END`), so only matching rows
    /// resolve a related object and the rest see `NULL`.
    #[tokio::test]
    async fn relation_field_is_masked_per_row() {
        let state = crate::utils::test_utils::get_app_state().await;
        let _ = inject_customers_meta(&state).await;

        let total = count_contacts(&state, "TRUE").await;
        let primary = count_contacts(&state, "is_primary = true").await;
        let non_primary = count_contacts(&state, "COALESCE(is_primary, false) = false").await;
        let (Some(total), Some(primary), Some(non_primary)) = (total, primary, non_primary) else {
            eprintln!("skipping: crm010production.contacts not seeded");
            return;
        };
        if total == 0 || primary == 0 || non_primary == 0 {
            eprintln!("skipping: contacts lack mixed is_primary rows");
            return;
        }

        let mut query = Query {
            fields: vec!["first_name".to_string(), "customer.name".to_string()],
            ..Default::default()
        };
        // Rule 0 grants `first_name` for every non-null name; rule 1 additionally
        // grants the `customer` relation but only for primary contacts.
        query.access = ReadAccess::Restricted {
            rules: vec![
                ReadRule {
                    fields: Some(vec!["first_name".to_string()]),
                    conditions: conditions_from_json(
                        &json!([{ "field": "first_name", "operator": "not_null" }]),
                    )
                    .unwrap(),
                    validations: vec![],
                    raw_validations: Value::Null,
                },
                ReadRule {
                    fields: Some(vec!["first_name".to_string(), "customer".to_string()]),
                    conditions: conditions_from_json(
                        &json!([{ "field": "is_primary", "operator": "eq", "value": true }]),
                    )
                    .unwrap(),
                    validations: vec![],
                    raw_validations: Value::Null,
                },
            ],
        };

        let ctx = crm_system_ctx();
        let rows = query
            .execute_query(&ctx, &state, &"contacts".to_string())
            .await
            .unwrap();
        assert_eq!(rows.len() as i64, total, "all rows are readable");

        let mut resolved = 0i64;
        for row in &rows {
            assert!(row.contains_key("first_name"), "first_name always granted");
            assert!(row.contains_key("customer"), "relation column emitted");
            if row.get("customer").map(|v| !v.is_null()).unwrap_or(false) {
                resolved += 1;
                let obj = row
                    .get("customer")
                    .and_then(Value::as_object)
                    .expect("resolved relation is an object");
                assert!(obj.contains_key("name"), "nested field fetched");
            }
        }
        assert_eq!(
            resolved, primary,
            "only primary contacts resolve a customer"
        );
        assert_eq!(
            total - resolved,
            non_primary,
            "non-primary contacts are masked to NULL"
        );
    }

    /// Test B: the caller's read rules on the *related* collection are enforced
    /// on the nested fetch, so fk values pointing at hidden rows become `NULL`.
    #[tokio::test]
    async fn nested_relation_read_enforces_target_rules() {
        let state = crate::utils::test_utils::get_app_state().await;
        if inject_customers_meta(&state).await.is_none() {
            eprintln!("skipping: crm010production.customers not seeded");
            return;
        }

        let user_id = sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE email = 'customer@acme.example'",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(user_id) = user_id else {
            eprintln!("skipping: customer@acme.example not seeded");
            return;
        };

        // The caller can see every customer they are a member of (which may be
        // more than Acme). Derive the visible set from `customers_users`.
        let visible_ids: Vec<String> = sqlx::query_scalar::<_, uuid::Uuid>(&format!(
            "SELECT customer FROM \"{CRM_SCHEMA}\".customers_users WHERE \"user\" = $1"
        ))
        .bind(user_id)
        .fetch_all(&*state.database_pool)
        .await
        .unwrap()
        .into_iter()
        .map(|id| id.to_string())
        .collect();
        let Some(_acme) = visible_ids.first().cloned() else {
            eprintln!("skipping: customer@acme.example has no customers_users membership");
            return;
        };
        let visible_literal = visible_ids
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let expected = count_contacts(&state, &format!("customer IN ({visible_literal})")).await;
        let non_acme =
            count_contacts(&state, &format!("customer NOT IN ({visible_literal})")).await;
        let (Some(expected), Some(non_acme)) = (expected, non_acme) else {
            eprintln!("skipping: crm010production.contacts not seeded");
            return;
        };
        if expected == 0 || non_acme == 0 {
            eprintln!("skipping: contacts lack mixed customer rows");
            return;
        }

        let mut query = Query {
            fields: vec!["first_name".to_string(), "customer.name".to_string()],
            ..Default::default()
        };
        // Parent unrestricted so every contact is returned.
        query.access = ReadAccess::Restricted {
            rules: vec![ReadRule {
                fields: None,
                conditions: vec![],
                validations: vec![],
                raw_validations: Value::Null,
            }],
        };

        let ctx = crm_ctx(Some(AuthLevel::User(user_id)));
        let rows = query
            .execute_query(&ctx, &state, &"contacts".to_string())
            .await
            .unwrap();

        let mut resolved = 0i64;
        let mut nulled = 0i64;
        for row in &rows {
            match row.get("customer") {
                Some(Value::Null) | None => nulled += 1,
                Some(Value::Object(obj)) => {
                    resolved += 1;
                    let id = obj.get("id").and_then(Value::as_str).unwrap_or_default();
                    assert!(
                        visible_ids.iter().any(|v| v == id),
                        "relation resolved a customer the caller cannot see: {id}"
                    );
                }
                Some(other) => panic!("unexpected relation value: {:?}", other),
            }
        }

        assert_eq!(
            resolved, expected,
            "contacts pointing at a visible customer resolve"
        );
        assert_eq!(
            nulled, non_acme,
            "contacts pointing at hidden customers are NULL"
        );
    }

    // ------------------------------------------------------------------
    // aggregate / groupBy
    // ------------------------------------------------------------------

    #[test]
    fn parses_aggregate_and_groupby_query_string() {
        let mut q: Query = serde_qs::from_str(
            "aggregate[count]=views,comments&aggregate[sum]=views&groupBy[]=author&groupBy[]=year(publish_date)&limit=5",
        )
        .unwrap();
        assert_eq!(
            q.group_by,
            vec!["author".to_string(), "year(publish_date)".to_string()]
        );
        assert_eq!(q.limit, 5);
        q.normalize_aggregate();
        assert_eq!(
            q.aggregate.get("count").unwrap().0,
            vec!["views".to_string(), "comments".to_string()]
        );
        assert_eq!(q.aggregate.get("sum").unwrap().0, vec!["views".to_string()]);
    }

    #[test]
    fn aggregate_expr_sql_mapping() {
        assert_eq!(
            parse_aggregate_expr("count", "*").unwrap().1,
            "COUNT(*)".to_string()
        );
        assert_eq!(
            parse_aggregate_expr("countAll", "id").unwrap().1,
            "COUNT(*)".to_string()
        );
        assert_eq!(
            parse_aggregate_expr("count", "views").unwrap(),
            (
                "views".to_string(),
                "COUNT(\"views\")".to_string(),
                "count_views".to_string()
            )
        );
        assert_eq!(
            parse_aggregate_expr("countDistinct", "views").unwrap().1,
            "COUNT(DISTINCT \"views\")".to_string()
        );
        assert_eq!(
            parse_aggregate_expr("sum", "views").unwrap().1,
            "SUM(\"views\")::float8".to_string()
        );
        assert_eq!(
            parse_aggregate_expr("avgDistinct", "views").unwrap().1,
            "AVG(DISTINCT \"views\")::float8".to_string()
        );
        assert_eq!(
            parse_aggregate_expr("min", "title").unwrap().1,
            "MIN(\"title\")".to_string()
        );
        assert!(parse_aggregate_expr("bogus", "views").is_err());
        assert!(parse_aggregate_expr("sum", "*").is_err());
    }

    #[test]
    fn group_expr_sql_mapping() {
        assert_eq!(
            parse_group_expr("state").unwrap(),
            (
                "state".to_string(),
                "\"state\"".to_string(),
                "state".to_string()
            )
        );
        assert_eq!(
            parse_group_expr("year(created_at)").unwrap(),
            (
                "created_at".to_string(),
                "EXTRACT(YEAR FROM \"created_at\")::int8".to_string(),
                "year_created_at".to_string()
            )
        );
        assert_eq!(
            parse_group_expr("date(created_at)").unwrap().1,
            "DATE_TRUNC('day', \"created_at\")".to_string()
        );
        assert_eq!(
            parse_group_expr("dow(created_at)").unwrap().1,
            "EXTRACT(DOW FROM \"created_at\")::int8".to_string()
        );
        assert!(parse_group_expr("bogus(x)").is_err());
        assert!(parse_group_expr("year(").is_err());
        assert!(parse_group_expr("*").is_err());
        assert!(parse_group_expr("year(*)").is_err());
    }

    const TEST_SCHEMA: &str = "testing010production";

    fn testing_ctx() -> AppContext {
        AppContext {
            app_name: "testing".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::SystemTest,
            identity: None,
            request_id: None,
        }
    }

    async fn testing_schema_seeded(state: &AppState) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'cases')",
        )
        .bind(TEST_SCHEMA)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap_or(false)
    }

    /// Client-side filter scoping a query to the seeded rows (matched by
    /// their unique `agg-<id>` titles), so leftover rows from other tests
    /// cannot skew aggregate counts.
    fn ids_filter(ids: &[String]) -> LogicOp {
        let titles: Vec<String> = ids.iter().map(|id| format!("agg-{id}")).collect();
        let mut map: HashMap<String, FieldValue> = HashMap::new();
        map.insert(
            "title".to_string(),
            FieldValue::Comparison(Comparison {
                _in: Some(json!(titles)),
                ..Default::default()
            }),
        );
        LogicOp {
            _and: Some(vec![Filter::Field(FieldFilter { fields: map })]),
            _or: None,
        }
    }

    /// Seeds three cases (two `open`, one `closed`) and returns their ids.
    async fn seed_cases(state: &AppState) -> Vec<String> {
        use crate::item_map;
        use crate::services::items::service::ItemsService;

        let ctx = testing_ctx();
        let collection = "cases".to_string();
        let mut service = ItemsService::new(state, &ctx, &collection);
        let ids: Vec<String> = (0..3).map(|_| uuid::Uuid::new_v4().to_string()).collect();
        let states = ["open", "open", "closed"];
        service
            .create_many(
                ids.iter()
                    .zip(states.iter())
                    .map(|(id, state)| {
                        item_map! {
                            "id" => id.clone(),
                            "title" => format!("agg-{id}"),
                            "state" => *state,
                        }
                    })
                    .collect(),
                &mut None,
            )
            .await
            .unwrap();
        ids
    }

    async fn cleanup_cases(state: &AppState, ids: &[String]) {
        for id in ids {
            sqlx::query(&format!(
                "DELETE FROM \"{TEST_SCHEMA}\".cases WHERE id = $1"
            ))
            .bind(uuid::Uuid::parse_str(id).unwrap())
            .execute(&*state.database_pool)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn aggregate_groupby_counts_per_group() {
        let state = crate::utils::test_utils::get_app_state().await;
        if !testing_schema_seeded(&state).await {
            eprintln!("skipping: {TEST_SCHEMA} not seeded");
            return;
        }
        let ids = seed_cases(&state).await;

        let mut query = Query {
            group_by: vec!["state".to_string()],
            aggregate: HashMap::from([("count".to_string(), FieldList(vec!["*".to_string()]))]),
            filter: ids_filter(&ids),
            ..Default::default()
        };
        let rows = query
            .execute_query(&testing_ctx(), &state, &"cases".to_string())
            .await
            .unwrap();

        let mut counts: HashMap<String, i64> = rows
            .iter()
            .map(|row| {
                (
                    row.get("state")
                        .and_then(Value::as_str)
                        .unwrap()
                        .to_string(),
                    row.get("count_*").and_then(Value::as_i64).unwrap(),
                )
            })
            .collect();
        assert_eq!(counts.remove("open"), Some(2));
        assert_eq!(counts.remove("closed"), Some(1));
        assert!(counts.is_empty(), "only the seeded groups: {counts:?}");

        // date-part grouping + field aggregate + sort on the alias
        let mut query = Query {
            group_by: vec!["year(created_at)".to_string()],
            aggregate: HashMap::from([("count".to_string(), FieldList(vec!["title".to_string()]))]),
            sort: vec!["-count_title".to_string()],
            filter: ids_filter(&ids),
            ..Default::default()
        };
        let rows = query
            .execute_query(&testing_ctx(), &state, &"cases".to_string())
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "all seeded rows share one created_at year");
        assert_eq!(rows[0].get("count_title").and_then(Value::as_i64), Some(3));
        assert!(rows[0].contains_key("year_created_at"));

        cleanup_cases(&state, &ids).await;
    }

    #[tokio::test]
    async fn aggregate_rejects_unknown_fields_and_injection() {
        let state = crate::utils::test_utils::get_app_state().await;
        if !testing_schema_seeded(&state).await {
            eprintln!("skipping: {TEST_SCHEMA} not seeded");
            return;
        }
        let ctx = testing_ctx();
        let table = "cases".to_string();

        let mut q = Query {
            aggregate: HashMap::from([("count".to_string(), FieldList(vec!["nope".to_string()]))]),
            ..Default::default()
        };
        let err = q.execute_query(&ctx, &state, &table).await.unwrap_err();
        assert!(matches!(err, AlcedoError::InvalidInput(_, _)), "{err}");

        let mut q = Query {
            group_by: vec!["id;DROP TABLE cases".to_string()],
            ..Default::default()
        };
        let err = q.execute_query(&ctx, &state, &table).await.unwrap_err();
        assert!(matches!(err, AlcedoError::InvalidInput(_, _)), "{err}");

        let mut q = Query {
            group_by: vec!["customer.name".to_string()],
            ..Default::default()
        };
        let err = q.execute_query(&ctx, &state, &table).await.unwrap_err();
        assert!(matches!(err, AlcedoError::InvalidInput(_, _)), "{err}");

        // the table must still be there
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'cases')",
        )
        .bind(TEST_SCHEMA)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert!(exists);
    }

    #[tokio::test]
    async fn aggregate_respects_restricted_field_access_and_row_filters() {
        let state = crate::utils::test_utils::get_app_state().await;
        if !testing_schema_seeded(&state).await {
            eprintln!("skipping: {TEST_SCHEMA} not seeded");
            return;
        }
        let ids = seed_cases(&state).await;
        let ctx = testing_ctx();
        let table = "cases".to_string();

        // rule grants only `title`: aggregating `state` is forbidden
        let mut q = Query {
            group_by: vec!["state".to_string()],
            aggregate: HashMap::from([("count".to_string(), FieldList(vec!["*".to_string()]))]),
            access: ReadAccess::Restricted {
                rules: vec![rule(Some(&["title"]))],
            },
            ..Default::default()
        };
        let err = q.execute_query(&ctx, &state, &table).await.unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "{err}");

        // `*` under a restricted rule set without an unrestricted rule is forbidden
        let mut q = Query {
            aggregate: HashMap::from([("count".to_string(), FieldList(vec!["*".to_string()]))]),
            access: ReadAccess::Restricted {
                rules: vec![rule(Some(&["title"]))],
            },
            ..Default::default()
        };
        let err = q.execute_query(&ctx, &state, &table).await.unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "{err}");

        // granted field + row condition: only `open` rows are counted
        let mut open_rule = rule(Some(&["title", "state"]));
        open_rule.conditions = conditions_from_json(&json!([
            { "field": "state", "operator": "eq", "value": "open" }
        ]))
        .unwrap();
        let mut q = Query {
            group_by: vec!["state".to_string()],
            aggregate: HashMap::from([("count".to_string(), FieldList(vec!["title".to_string()]))]),
            filter: ids_filter(&ids),
            access: ReadAccess::Restricted {
                rules: vec![open_rule],
            },
            ..Default::default()
        };
        let rows = q.execute_query(&ctx, &state, &table).await.unwrap();
        assert_eq!(rows.len(), 1, "row filter limits the groups");
        assert_eq!(rows[0].get("state").and_then(Value::as_str), Some("open"));
        assert_eq!(rows[0].get("count_title").and_then(Value::as_i64), Some(2));

        cleanup_cases(&state, &ids).await;
    }
}
