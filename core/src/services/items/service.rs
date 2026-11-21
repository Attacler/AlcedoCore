use sea_query::{Alias, Expr, PostgresQueryBuilder, SimpleExpr};
use serde_json::{Map, Value};
use sqlx::{Postgres, Row, Transaction};
use std::collections::{HashMap, HashSet};

use crate::services::collections::ddl::{qtable, quote};
use crate::services::collections::schema::get_pk_key;
use crate::services::context::AppContext;
use crate::services::hooks::HookContext;
use crate::services::hooks::types::items_create::{ItemsAfterCreate, ItemsBeforeCreate};
use crate::services::hooks::types::items_delete::{ItemsAfterDelete, ItemsBeforeDelete};
use crate::services::hooks::types::items_update::{ItemsAfterUpdate, ItemsBeforeUpdate};
use crate::services::items::query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query};
use crate::services::permissions;
use crate::services::permissions::read::{ReadAccess, resolve_access, resolve_read_access};
use crate::services::postgres::jsonvalue_simpleexpr::parse_value;
use crate::services::postgres::pool::{
    execute_query, execute_query_transaction, pgrow_to_json, process_query_error_response,
};
use crate::{AppState, services::errors::AlcedoError};
use tokio::sync::OnceCell;

/// Rows per set-based DELETE statement (`WHERE pk = ANY($1)`).
const DELETE_CHUNK: usize = 1000;
/// Rows per set-based UPDATE (`... FROM (VALUES ...) AS v(pk)`).
const UPDATE_CHUNK: usize = 500;
/// Rows per multi-row INSERT (`VALUES (...), (...)`).
const INSERT_CHUNK: usize = 500;

/// Renders a `SimpleExpr` (literal, cast, custom SQL) as a standalone Postgres
/// fragment by wrapping it in a `SELECT` and stripping the prefix.
fn render_simple_expr(expr: &SimpleExpr) -> String {
    let mut stmt = sea_query::Query::select();
    stmt.expr(expr.clone());
    let sql = stmt.to_string(PostgresQueryBuilder);
    sql.strip_prefix("SELECT ").unwrap_or(&sql).to_string()
}

/// The array cast applied to the bound `Vec<String>` of pk values in
/// `pk = ANY($1...)`, chosen from the pk column's type so Postgres coerces the
/// text array to the pk's array type.
fn pk_array_cast(data_type: &str) -> &'static str {
    match data_type {
        "uuid" => "::uuid[]",
        "bigint" | "bigserial" => "::bigint[]",
        "integer" | "int" | "int4" | "serial4" => "::int[]",
        "smallint" | "int2" | "serial2" => "::smallint[]",
        "real" | "float4" => "::float4[]",
        "double precision" | "float8" | "float" => "::float8[]",
        "boolean" => "::boolean[]",
        "numeric" | "decimal" | "money" => "::numeric[]",
        "date" => "::date[]",
        "time" | "time without time zone" => "::time[]",
        "timetz" | "time with time zone" => "::timetz[]",
        "timestamp" | "timestamp without time zone" => "::timestamp[]",
        "timestamptz" | "timestamp with time zone" => "::timestamptz[]",
        _ => "::text[]",
    }
}

/// The bindable (text) form of a pk value. `parse_pk_value` has already
/// validated the value, so only strings/numbers/bools reach this.
fn pk_bind_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub struct ItemsService<'a> {
    app_state: &'a AppState,
    collection: &'a String,
    app_context: &'a AppContext,
    /// Lazily resolved, per-service cache of the caller's record-level read
    /// access for `collection`.
    resolved_access: OnceCell<ReadAccess>,
    /// Lazily resolved, per-service cache of the caller's record-level `delete`
    /// access for `collection`.
    resolved_delete_access: OnceCell<ReadAccess>,
    /// `after`-write events deferred until the caller-owned transaction commits
    /// (see [`ItemsService::run_after_commit`]).
    pending_after: Vec<PendingAfter>,
}

/// A deferred `after`-write hook: the event, its trigger key, and the app
/// context it belongs to. A nested (cross-app) write can target a different
/// schema, so every deferred event carries its own context rather than relying
/// on the context of whichever service drains the queue.
pub enum PendingAfter {
    Create {
        key: String,
        context: AppContext,
        event: ItemsAfterCreate,
    },
    Update {
        key: String,
        context: AppContext,
        event: ItemsAfterUpdate,
    },
    Delete {
        key: String,
        context: AppContext,
        event: ItemsAfterDelete,
    },
}

/// Collects the `after`-write hooks deferred by writes made inside a
/// caller-owned transaction. The transaction's owner must call
/// [`AfterCommitQueue::run`] **after committing**, so the hooks observe the
/// committed rows. Used by the recursive relational writers, whose individual
/// `ItemsService` instances are transient.
#[derive(Default)]
struct AfterCommitQueue {
    pending: Vec<PendingAfter>,
}

impl AfterCommitQueue {
    /// Merges the deferred events of a transient service into this queue.
    fn append(&mut self, mut other: Vec<PendingAfter>) {
        self.pending.append(&mut other);
    }

    /// Triggers every deferred `after` hook. Takes the queue, so a second call
    /// is a no-op.
    async fn run(&mut self, state: &AppState) {
        let pending = std::mem::take(&mut self.pending);
        for deferred in pending {
            match deferred {
                PendingAfter::Create {
                    key,
                    context,
                    mut event,
                } => {
                    let hook_context = HookContext {
                        context: context.clone(),
                        state: state.clone(),
                        tx: None,
                    };
                    state
                        .event_bus
                        .trigger(&key, &mut event, hook_context)
                        .await;
                    // Also fire the collection-scoped key so hooks registered on
                    // `after.items.*.{collection}` only run for their collection.
                    let scoped_key = format!("{}.{}", key, event.collection);
                    state
                        .event_bus
                        .trigger(
                            &scoped_key,
                            &mut event,
                            HookContext {
                                context: context.clone(),
                                state: state.clone(),
                                tx: None,
                            },
                        )
                        .await;
                }
                PendingAfter::Update {
                    key,
                    context,
                    mut event,
                } => {
                    let hook_context = HookContext {
                        context: context.clone(),
                        state: state.clone(),
                        tx: None,
                    };
                    state
                        .event_bus
                        .trigger(&key, &mut event, hook_context)
                        .await;
                    // Also fire the collection-scoped key so hooks registered on
                    // `after.items.*.{collection}` only run for their collection.
                    let scoped_key = format!("{}.{}", key, event.collection);
                    state
                        .event_bus
                        .trigger(
                            &scoped_key,
                            &mut event,
                            HookContext {
                                context: context.clone(),
                                state: state.clone(),
                                tx: None,
                            },
                        )
                        .await;
                }
                PendingAfter::Delete {
                    key,
                    context,
                    mut event,
                } => {
                    let hook_context = HookContext {
                        context: context.clone(),
                        state: state.clone(),
                        tx: None,
                    };
                    state
                        .event_bus
                        .trigger(&key, &mut event, hook_context)
                        .await;
                    // Also fire the collection-scoped key so hooks registered on
                    // `after.items.*.{collection}` only run for their collection.
                    let scoped_key = format!("{}.{}", key, event.collection);
                    state
                        .event_bus
                        .trigger(
                            &scoped_key,
                            &mut event,
                            HookContext {
                                context: context.clone(),
                                state: state.clone(),
                                tx: None,
                            },
                        )
                        .await;
                }
            }
        }
    }
}

/// Owns a caller-controlled transaction together with the `after`-write hooks
/// deferred inside it.
///
/// Writers register their deferred events with the guard (via
/// [`append`](Self::append)) instead of threading a separate queue. On
/// [`commit`](Self::commit) the transaction commits and the events dispatch; if
/// the guard is dropped **without** committing (error, `?`, or panic) sqlx rolls
/// the transaction back and the pending events are discarded. So the events can
/// never fire for a rolled-back write, and nothing leaks.
///
/// Not an `Iterator`/`Deref`: the tx is reached through [`tx`](Self::tx) so the
/// guard stays the single owner.
pub struct TxGuard<'c> {
    tx: Transaction<'c, Postgres>,
    after: AfterCommitQueue,
    /// Set via [`mark_dirty`](Self::mark_dirty) when the transaction performed
    /// DDL (schema changes). On commit, the schema is only marked dirty when
    /// this flag is set, so plain item writes don't defeat the dirty-generation
    /// coalescing in `SchemaService::refresh_schema`.
    dirty: bool,
}

impl<'c> TxGuard<'c> {
    pub fn new(tx: Transaction<'c, Postgres>) -> Self {
        Self {
            tx,
            after: AfterCommitQueue::default(),
            dirty: false,
        }
    }

    /// The underlying transaction, for the query builders that expect it.
    pub fn tx(&mut self) -> &mut Transaction<'c, Postgres> {
        &mut self.tx
    }

    /// Merges a transient service's deferred events into this transaction.
    pub fn append(&mut self, pending: Vec<PendingAfter>) {
        self.after.append(pending);
    }

    /// Marks this transaction as having performed DDL, so that
    /// [`commit`](Self::commit)/[`commit_sync`](Self::commit_sync) mark the
    /// schema dirty on commit.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Commits, then dispatches the deferred `after` hooks in a background
    /// task, so a slow `after` hook cannot block the response. Consumes the
    /// guard, so the events fire at most once. Use [`Self::commit_sync`] where
    /// the caller must wait for the hooks (tests).
    ///
    /// The schema is marked dirty on commit only if the guard was flagged via
    /// [`mark_dirty`](Self::mark_dirty) (i.e. the transaction did DDL); plain
    /// item writes leave the schema-dirty generation untouched.
    ///
    /// Note: cache-invalidation hooks (permissions, schema) therefore run a
    /// few ms after the response — a concurrent request can briefly serve a
    /// stale cached permission entry. Accepted trade-off for not blocking on
    /// arbitrary hook work.
    pub async fn commit(self, state: &AppState) -> Result<(), AlcedoError> {
        let TxGuard { tx, after, dirty } = self;
        tx.commit().await?;
        if dirty {
            state.mark_schema_dirty();
        }
        let state = state.clone();
        tokio::spawn(async move {
            let mut after = after;
            after.run(&state).await;
        });
        Ok(())
    }

    /// Commits, then awaits the deferred `after` hooks inline.
    pub async fn commit_sync(self, state: &AppState) -> Result<(), AlcedoError> {
        let TxGuard { tx, mut after, dirty } = self;
        tx.commit().await?;
        if dirty {
            state.mark_schema_dirty();
        }
        after.run(state).await;
        Ok(())
    }
}

/// A pk as a plain map key (strings unquoted, everything else rendered as-is).
fn json_value_to_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn pk_value_to_string(v: &Value) -> Result<String, AlcedoError> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Null => Err(AlcedoError::SystemError(
            "Insert/update did not return a primary key".to_string(),
            1,
        )),
        other => Err(AlcedoError::SystemError(
            format!("Unexpected primary key type: {}", other),
            1,
        )),
    }
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
            resolved_access: OnceCell::new(),
            resolved_delete_access: OnceCell::new(),
            pending_after: Vec::new(),
        };
    }

    /// Triggers every `after`-write hook deferred by an external-transaction
    /// write, in a background task so a slow hook cannot block the caller. The
    /// caller must call this **after committing its transaction**, so the
    /// hooks observe the committed rows. Use [`Self::run_after_commit_sync`]
    /// where the caller must wait for the hooks (tests).
    pub async fn run_after_commit(&mut self) {
        let mut queue = AfterCommitQueue {
            pending: std::mem::take(&mut self.pending_after),
        };
        let state = self.app_state.clone();
        tokio::spawn(async move {
            queue.run(&state).await;
        });
    }

    /// Awaits every deferred `after`-write hook inline.
    pub async fn run_after_commit_sync(&mut self) {
        let mut queue = AfterCommitQueue {
            pending: std::mem::take(&mut self.pending_after),
        };
        queue.run(self.app_state).await;
    }

    /// Moves this service's deferred `after` hooks into a caller-owned queue.
    /// Used by the transient services the recursive relational writers build.
    pub fn take_pending_after(&mut self) -> Vec<PendingAfter> {
        std::mem::take(&mut self.pending_after)
    }

    /// Defers an `after.items.create` hook to the commit boundary. The
    /// activity-log system hook observes this event and queues log rows via
    /// the async channel.
    ///
    /// `pk_name` is the primary-key column: `create_items_with_tx` injects it
    /// into each item, so hooks can carry the item id.
    fn defer_after_create(&mut self, items: Vec<Map<String, Value>>, pk_name: String) {
        self.pending_after.push(PendingAfter::Create {
            key: "after.items.create".to_string(),
            context: self.app_context.clone(),
            event: ItemsAfterCreate {
                items,
                collection: self.collection.clone(),
                pk_name,
            },
        });
    }

    /// Defers an `after.items.update` hook to the commit boundary. The
    /// activity-log system hook builds field-level diffs from `old_items`.
    fn defer_after_update(
        &mut self,
        keys: Vec<String>,
        payload: Map<String, Value>,
        old_items: Vec<Map<String, Value>>,
        _pk_name: String,
    ) {
        self.pending_after.push(PendingAfter::Update {
            key: "after.items.update".to_string(),
            context: self.app_context.clone(),
            event: ItemsAfterUpdate {
                keys,
                payload,
                collection: self.collection.clone(),
                old_items,
            },
        });
    }

    /// Defers an `after.items.delete` hook to the commit boundary. The
    /// activity-log system hook builds delete diffs from `old_rows`.
    fn defer_after_delete(&mut self, keys: Vec<Value>, old_rows: Vec<Map<String, Value>>) {
        self.pending_after.push(PendingAfter::Delete {
            key: "after.items.delete".to_string(),
            context: self.app_context.clone(),
            event: ItemsAfterDelete {
                keys,
                collection: self.collection.clone(),
                old_rows,
            },
        });
    }

    /// Resolves (and caches) the caller's record-level read access for this
    /// service's collection.
    async fn read_access(&self) -> Result<&ReadAccess, AlcedoError> {
        self.resolved_access
            .get_or_try_init(|| async {
                resolve_read_access(
                    self.app_state,
                    self.app_context,
                    self.collection,
                    self.app_context.identity.as_ref(),
                )
                .await
            })
            .await
    }

    async fn delete_access(&self) -> Result<&ReadAccess, AlcedoError> {
        self.resolved_delete_access
            .get_or_try_init(|| async {
                resolve_access(
                    self.app_state,
                    self.app_context,
                    self.collection,
                    self.app_context.identity.as_ref(),
                    "delete",
                )
                .await
            })
            .await
    }

    pub async fn read_items_by_query(
        &self,
        mut query: Query,
    ) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        // `Box::pin` breaks the async recursion cycle
        // (`resolve_read_access` -> `AuthService::is_admin` -> items read).
        query.access = Box::pin(self.read_access()).await?.clone();
        Ok(query
            .execute_query(self.app_context, self.app_state, self.collection)
            .await?)
    }

    pub async fn count_items_by_query(&self, mut query: Query) -> Result<i64, AlcedoError> {
        query.limit = 0;
        query.offset = 0;
        query.access = Box::pin(self.read_access()).await?.clone();
        if matches!(query.access, ReadAccess::Deny) {
            return Ok(0);
        }
        // `count` bypasses `execute_query`, so it must apply the access filter
        // itself before the relation hops and SQL are resolved.
        query.apply_access();
        query.relation_hops = Box::pin(
            crate::services::items::filter_relations::resolve_filter_relation_hops(
                self.app_state,
                self.app_context,
                self.collection,
                &query.filter,
            ),
        )
        .await?;
        let inner = {
            let schema = self.app_state.database_schema.read().await;
            let (stmt, _) = query.to_sql(self.collection, &schema, self.app_context)?;
            stmt.to_string(PostgresQueryBuilder)
        };
        let count_sql = format!(
            "SELECT COUNT(*)::bigint AS count FROM ({}) AS _alcedocore_count",
            inner
        );
        let rows = execute_query(self.app_state, count_sql).await?;
        Ok(rows
            .get(0)
            .and_then(|row| row.try_get("count").ok())
            .unwrap_or(0))
    }

    /// Maps each pk in `pks` (max 100) to whether the caller may delete it,
    /// mirroring exactly what `delete_items_by_query` enforces.
    ///
    /// `Unrestricted` (admin / dev key / framework collection) is true for every
    /// pk; `Deny` is false for every pk; otherwise the delete-rule filter is
    /// run once over the pk set and its survivors are true.
    pub async fn delete_permissions_for_pks(
        &self,
        pks: &[Value],
    ) -> Result<Map<String, Value>, AlcedoError> {
        const MAX_PKS: usize = 100;
        if pks.len() > MAX_PKS {
            return Err(AlcedoError::InvalidInput(
                format!("At most {} pks may be checked at once", MAX_PKS),
                0,
            ));
        }

        let mut result: Map<String, Value> = Map::new();
        if pks.is_empty() {
            return Ok(result);
        }

        match self.delete_access().await? {
            ReadAccess::Unrestricted => {
                for pk in pks {
                    result.insert(json_value_to_key(pk), Value::Bool(true));
                }
                return Ok(result);
            }
            ReadAccess::Deny => {
                for pk in pks {
                    result.insert(json_value_to_key(pk), Value::Bool(false));
                }
                return Ok(result);
            }
            access => {
                let pk_name = get_pk_key(
                    &self.app_state.database_schema,
                    &self.app_context.schema_name(),
                    &self.collection,
                )
                .await?
                .name;

                let mut pk_filter = FieldFilter {
                    fields: HashMap::new(),
                };
                pk_filter.fields.insert(
                    pk_name.clone(),
                    FieldValue::Comparison(Comparison {
                        _in: Some(Value::Array(pks.to_vec())),
                        ..Default::default()
                    }),
                );

                let mut query = Query {
                    fields: vec![pk_name.clone()],
                    filter: LogicOp {
                        _and: Some(vec![Filter::Field(pk_filter)]),
                        _or: None,
                    },
                    limit: 0,
                    ..Default::default()
                };
                query.access = access.clone();
                let allowed: HashSet<String> = query
                    .execute_query(self.app_context, self.app_state, &self.collection)
                    .await?
                    .iter()
                    .filter_map(|row| row.get(&pk_name))
                    .map(json_value_to_key)
                    .collect();

                for pk in pks {
                    let key = json_value_to_key(pk);
                    result.insert(key.clone(), Value::Bool(allowed.contains(&key)));
                }
                Ok(result)
            }
        }
    }

    pub async fn get_items_by_pks<'a>(
        &self,
        pks: Vec<Value>,
    ) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        let pk = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?;
        let mut query = Query::default();
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
        self.read_items_by_query(query).await
    }

    pub async fn get_single_item_by_pk(
        &self,
        pk: Value,
    ) -> Result<Option<Map<String, Value>>, AlcedoError> {
        Ok(self.get_items_by_pks(vec![pk]).await?.into_iter().next())
    }

    pub async fn update_items_by_query<'a>(
        &mut self,
        query: &mut Query,
        payload: Map<String, Value>,
        database_transaction: &'a mut Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<Vec<String>, AlcedoError> {
        match database_transaction {
            Some(existing_tx) => {
                let (keys, payload, old_items, pk_name) = self
                    .update_items_with_tx(existing_tx, query, payload)
                    .await?;
                // The caller owns the commit, so defer the `after` hook until
                // `run_after_commit` (called by that caller post-commit).
                self.defer_after_update(keys.clone(), payload, old_items, pk_name);
                Ok(keys)
            }
            None => {
                let mut tx = self.app_state.database_pool.begin().await?;
                let (keys, payload, old_items, pk_name) =
                    self.update_items_with_tx(&mut tx, query, payload).await?;
                tx.commit().await?;
                // `after` hooks fire post-commit: a hook reading through the
                // pool (schema refresh, cache invalidation) must see the write.
                self.defer_after_update(keys.clone(), payload, old_items, pk_name);
                self.run_after_commit().await;
                Ok(keys)
            }
        }
    }
    async fn update_items_with_tx<'a>(
        &self,
        tx: &'a mut Transaction<'_, Postgres>,
        query: &mut Query,
        payload: Map<String, Value>,
    ) -> Result<
        (
            Vec<String>,
            Map<String, Value>,
            Vec<Map<String, Value>>,
            String,
        ),
        AlcedoError,
    > {
        let pk_name = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?
        .name;

        // Read the affected rows in full (not just their pk) so the activity log
        // can diff old vs new. `ItemsBeforeUpdate.keys` filters on `pk_name`
        // below, so the extra columns are harmless to the hook.
        query.ensure_pk_selected(&pk_name);
        let get_pks = query
            .execute_query(self.app_context, self.app_state, self.collection)
            .await?;
        let old_items = get_pks.clone();

        let mut before = ItemsBeforeUpdate {
            keys: get_pks
                .iter()
                .filter_map(|item| item.get(&pk_name))
                .map(json_value_to_key)
                .collect(),
            payload,
            collection: self.collection.clone(),
        };

        {
            let hook_context = HookContext {
                context: self.app_context.clone(),
                state: self.app_state.clone(),
                tx: Some(tx),
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

        // Record-level update policy, checked before any write. Three
        // questions, in order: may the caller touch these rows (row filter),
        // may they change these keys (delta whitelist), and is the result
        // allowed (value check). All three share the `update` action's rules,
        // and the row filter is resolved once for both checks.
        let target_pks: Vec<Value> = get_pks
            .iter()
            .filter_map(|item| item.get(&pk_name).cloned())
            .collect();
        let update_access = crate::services::permissions::update::resolve_update(
            self.app_state,
            self.app_context,
            self.collection,
            &target_pks,
        )
        .await?;
        crate::services::permissions::update::check_update(
            self.app_state,
            self.app_context,
            self.collection,
            &update_access,
            &before.payload,
        )
        .await?;

        // The payload is identical for every row, so validate/render the SET
        // clause once and drive the write with a single set-based statement
        // per chunk of pks instead of one UPDATE per row.
        let mut update_items: Vec<String> = vec![];
        if !get_pks.is_empty() {
            let set_exprs = self.generate_update_set_exprs(&before.payload).await?;

            // Validate every pk up front (same error the per-row builder
            // produced) so a bad value aborts before any UPDATE runs.
            let mut errors: Vec<String> = vec![];
            let mut pk_exprs: Vec<String> = Vec::with_capacity(get_pks.len());
            for item in &get_pks {
                let pk_val = item.get(&pk_name).unwrap().clone();
                match self.parse_pk_value(&pk_name, &pk_val).await {
                    Ok(expr) => pk_exprs.push(render_simple_expr(&expr)),
                    Err(e) => errors.push(e.to_string()),
                }
            }
            if !errors.is_empty() {
                return Err(AlcedoError::InvalidInput(errors.join(","), 1));
            }

            let qtable = qtable(self.app_context, &self.collection);
            let pk_col = quote(&pk_name);
            let set_clause = set_exprs
                .iter()
                .map(|(field, expr)| format!("{} = {}", quote(field), render_simple_expr(expr)))
                .collect::<Vec<_>>()
                .join(", ");

            let mut updated: HashSet<String> = HashSet::new();
            for chunk in pk_exprs.chunks(UPDATE_CHUNK) {
                let values_list = chunk
                    .iter()
                    .map(|pk| format!("({})", pk))
                    .collect::<Vec<_>>()
                    .join(", ");
                let query_str = format!(
                    "UPDATE {qtable} SET {set_clause} FROM (VALUES {values_list}) AS \"v\"({pk_col}) \
                     WHERE {qtable}.{pk_col} = \"v\".{pk_col} RETURNING {qtable}.{pk_col}"
                );
                let result = execute_query_transaction(&self.app_state, tx, &query_str).await?;
                for row in &result {
                    let pk_data = pgrow_to_json(row).unwrap();
                    updated.insert(pk_value_to_string(pk_data.get(&pk_name).unwrap())?);
                }
            }

            // Keep the caller-visible key order (the pk-selection query's).
            update_items = get_pks
                .iter()
                .filter_map(|item| item.get(&pk_name))
                .filter_map(|v| pk_value_to_string(v).ok())
                .filter(|key| updated.contains(key))
                .collect();
        }

        Ok((update_items, before.payload, old_items, pk_name))
    }

    pub async fn create_many<'a>(
        &mut self,
        items: Vec<Map<String, Value>>,
        database_transaction: &'a mut Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<Vec<String>, AlcedoError> {
        match database_transaction {
            Some(existing_tx) => {
                let (keys, items, pk_name) = self.create_items_with_tx(existing_tx, items).await?;
                self.defer_after_create(items, pk_name);
                Ok(keys)
            }
            None => {
                let mut tx = self.app_state.database_pool.begin().await?;
                let (keys, items, pk_name) = self.create_items_with_tx(&mut tx, items).await?;
                tx.commit().await?;
                // `after` hooks fire post-commit (see `update_items_by_query`).
                self.defer_after_create(items, pk_name);
                self.run_after_commit().await;
                Ok(keys)
            }
        }
    }

    async fn create_items_with_tx<'a>(
        &self,
        tx: &'a mut Transaction<'_, Postgres>,
        items: Vec<Map<String, Value>>,
    ) -> Result<(Vec<String>, Vec<Map<String, Value>>, String), AlcedoError> {
        let mut before = ItemsBeforeCreate {
            items,
            collection: self.collection.clone(),
        };

        let hook_context = HookContext {
            context: self.app_context.clone(),
            state: self.app_state.clone(),
            tx: Some(tx),
        };

        self.app_state
            .event_bus
            .trigger(
                &format!("before.items.create.{}", self.collection),
                &mut before,
                hook_context,
            )
            .await;

        let create_access = permissions::create::resolve_create_access(
            self.app_state,
            self.app_context,
            self.collection,
            self.app_context.identity.as_ref(),
        )
        .await?;
        permissions::create::check_create(
            self.app_state,
            self.app_context,
            self.collection,
            &create_access,
            &before.items,
        )
        .await?;

        let pk_name = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?
        .name;
        let pk_name_cloned = pk_name.clone();

        // Union of columns across all items (first-seen order); a column an
        // item omits is inserted as DEFAULT, exactly like the column being
        // left out of that item's single-row INSERT was.
        let mut columns: Vec<String> = vec![];
        let mut seen: HashSet<&str> = HashSet::new();
        for item in &before.items {
            for field in item.keys() {
                if seen.insert(field.as_str()) {
                    columns.push(field.clone());
                }
            }
        }

        // Validate every item and render its row of values BEFORE executing
        // anything: a bad item aborts the whole batch, like the previous
        // per-row validation did.
        let mut rows: Vec<Vec<SimpleExpr>> = Vec::with_capacity(before.items.len());
        let mut errors: Vec<String> = vec![];
        for item in &before.items {
            match self.generate_insert_item_row(item, &columns).await {
                Ok(row) => rows.push(row),
                Err(e) => errors.push(e.to_string()),
            }
        }
        if !errors.is_empty() {
            return Err(AlcedoError::InvalidInput(errors.join(","), 1));
        }

        // One multi-row INSERT per chunk of items instead of one INSERT per item.
        let mut statements: Vec<String> = vec![];
        for chunk in rows.chunks(INSERT_CHUNK) {
            let mut stmt = sea_query::Query::insert();
            stmt.into_table((
                Alias::new(self.app_context.schema_name()),
                Alias::new(self.collection),
            ));
            stmt.columns(columns.iter().map(|col| Alias::new(col.clone())));
            for row in chunk {
                let _ = stmt.values(row.clone());
            }
            stmt.returning_col(Alias::new(pk_name.clone()));
            statements.push(stmt.to_string(PostgresQueryBuilder));
        }

        let mut created_items = vec![];
        for query in statements {
            let result = execute_query_transaction(&self.app_state, tx, &query).await?;
            for row in &result {
                let pk = pgrow_to_json(row).unwrap();
                let pk = pk_value_to_string(pk.get(&pk_name).unwrap())?;
                created_items.push(pk);
            }
        }

        before
            .items
            .iter_mut()
            .enumerate()
            .for_each(|(index, item)| {
                let pk = created_items[index].clone();
                item.insert(pk_name_cloned.clone(), pk.into());
            });

        Ok((created_items, before.items, pk_name_cloned))
    }

    pub async fn delete_items_by_pks<'a>(
        &mut self,
        pks: Vec<Value>,
        mut database_transaction: Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<u64, AlcedoError> {
        let pk = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?;
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
        &mut self,
        query: Query,
        database_transaction: &'a mut Option<&'a mut Transaction<'_, Postgres>>,
    ) -> Result<u64, AlcedoError> {
        match database_transaction {
            Some(existing_tx) => {
                let (deleted, keys, old_rows) = self
                    .delete_items_by_query_with_tx(existing_tx, query)
                    .await?;
                self.defer_after_delete(keys, old_rows);
                Ok(deleted)
            }
            None => {
                let mut tx = self.app_state.database_pool.begin().await?;
                let (deleted, keys, old_rows) =
                    self.delete_items_by_query_with_tx(&mut tx, query).await?;
                tx.commit().await?;
                // `after` hooks fire post-commit (see `update_items_by_query`).
                self.defer_after_delete(keys, old_rows);
                self.run_after_commit().await;
                Ok(deleted)
            }
        }
    }

    pub async fn delete_items_by_query_with_tx<'a>(
        &self,
        tx: &'a mut Transaction<'_, Postgres>,
        mut query: Query,
    ) -> Result<(u64, Vec<Value>, Vec<Map<String, Value>>), AlcedoError> {
        // Record-level delete policy: a caller with no matching `delete` rule is
        // rejected outright; otherwise the rules are AND-ed into the
        // pk-selection query below, so out-of-policy rows are never deleted.
        match self.delete_access().await? {
            ReadAccess::Deny => {
                return Err(AlcedoError::Forbidden(
                    format!(
                        "You do not have permission to delete items from '{}'",
                        self.collection
                    ),
                    0,
                ));
            }
            access => query.access = access.clone(),
        }

        let hook_context = HookContext {
            context: self.app_context.clone(),
            state: self.app_state.clone(),
            tx: Some(tx),
        };

        let pk_name = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?
        .name;

        // Keep the full old rows (not just the pk) so the activity log can
        // record a field-level diff for each deleted item.
        query.ensure_pk_selected(&pk_name);
        let old_rows = query
            .execute_query(self.app_context, self.app_state, self.collection)
            .await?;

        let get_pks = old_rows
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
                &format!("before.items.delete.{}", self.collection),
                &mut before,
                hook_context,
            )
            .await;

        // Validate every pk up front (same error the per-row builder produced)
        // so a bad value aborts before any DELETE runs.
        let mut errors: Vec<String> = vec![];
        let mut pk_binds: Vec<String> = Vec::with_capacity(before.keys.len());
        for pk in &before.keys {
            match self.parse_pk_value(&pk_name, pk).await {
                Ok(_) => pk_binds.push(pk_bind_string(pk)),
                Err(e) => errors.push(e.to_string()),
            }
        }
        if !errors.is_empty() {
            return Err(AlcedoError::InvalidInput(errors.join(","), 1));
        }

        // One set-based DELETE per chunk of pks instead of one DELETE per pk.
        let pk_column = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?;
        let qtable = qtable(self.app_context, &self.collection);
        let query = format!(
            "DELETE FROM {qtable} WHERE {} = ANY($1{})",
            quote(&pk_name),
            pk_array_cast(&pk_column.data_type),
        );

        let mut total_deleted = 0;
        for chunk in pk_binds.chunks(DELETE_CHUNK) {
            let insert = sqlx::query(&query).bind(chunk.to_vec()).execute(&mut **tx).await;
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

        Ok((total_deleted, before.keys, old_rows))
    }

    /// Validates one item against the schema and renders its row of values
    /// aligned to `columns` (the union of columns across the batch). A column
    /// the item omits — or whose value fails to parse but is nullable /
    /// auto-increment — becomes `DEFAULT`, matching the old behavior of
    /// leaving the column out of that item's single-row INSERT.
    async fn generate_insert_item_row(
        &self,
        item: &Map<String, Value>,
        columns: &[String],
    ) -> Result<Vec<SimpleExpr>, AlcedoError> {
        let mut values: Vec<SimpleExpr> = Vec::with_capacity(columns.len());
        {
            let schema = self.app_state.database_schema.read().await;
            for field in columns {
                let Some(value) = item.get(field) else {
                    values.push(Expr::cust("DEFAULT"));
                    continue;
                };
                let column = schema.columns.iter().find(|col| {
                    &col.table == self.collection
                        && col.name == *field
                        && col.schema == self.app_context.schema_name()
                });

                if let None = column {
                    return Err(AlcedoError::InvalidInput(
                        format!("Unknown field {} provided", field),
                        1,
                    ));
                }
                let value = match parse_value(
                    &schema,
                    &self.app_context.schema_name(),
                    &vec![],
                    &self.collection,
                    field,
                    value.clone(),
                ) {
                    None => {
                        if let Some(column) = column {
                            if column.is_nullable || column.has_auto_increment {
                                values.push(Expr::cust("DEFAULT"));
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

                values.push(value);
            }
        }
        Ok(values)
    }

    /// Validates the (row-independent) update payload against the schema and
    /// renders the `SET` expressions for the set-based UPDATE. Same checks as
    /// the old per-row builder: unknown field, or a value that fails to parse
    /// on a non-nullable field, is an error; on a nullable field it becomes
    /// NULL.
    async fn generate_update_set_exprs(
        &self,
        item: &Map<String, Value>,
    ) -> Result<Vec<(String, SimpleExpr)>, AlcedoError> {
        let mut values: Vec<(String, SimpleExpr)> = vec![];

        {
            // Scoped so the read guard is dropped before any await below:
            // re-entering `read()` on the same RwLock while holding the guard
            // deadlocks once a writer is queued.
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
                let value = match parse_value(
                    &schema,
                    &self.app_context.schema_name(),
                    &vec![],
                    &self.collection,
                    &field,
                    value,
                ) {
                    None => {
                        let is_nullable = column.map_or(false, |c| c.is_nullable);

                        if is_nullable {
                            let val: Option<i32> = None;
                            let expr = Expr::value(val);
                            expr
                        } else {
                            return Err(AlcedoError::InvalidInput(
                                format!(
                                    "Field {} is not nullable and received invalid input",
                                    field
                                ),
                                1,
                            ));
                        }
                    }
                    Some(val) => val,
                };

                values.push((field, value));
            }
        }
        Ok(values)
    }

    async fn parse_pk_value(
        &self,
        pk_name: &str,
        pk_val: &Value,
    ) -> Result<SimpleExpr, AlcedoError> {
        let schema = self.app_state.database_schema.read().await;
        parse_value(
            &schema,
            &self.app_context.schema_name(),
            &vec![],
            &self.collection,
            pk_name,
            pk_val.clone(),
        )
        .ok_or_else(|| {
            AlcedoError::InvalidInput(
                format!("Invalid primary key value for field {}", pk_name),
                1,
            )
        })
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

#[cfg(test)]
mod after_commit_tests {
    use std::sync::{Arc, Mutex};

    use uuid::Uuid;

    use super::*;
    use crate::item_map;
    use crate::services::context::{AppContext, RequestSource};
    use crate::services::hooks::types::items_create::ItemsAfterCreate;

    const SCHEMA: &str = "crm010production";

    fn ctx() -> AppContext {
        AppContext {
            app_name: "crm".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity: None,
            request_id: None,
        }
    }

    /// A write inside a caller-owned transaction must defer its `after` hook
    /// (with the created rows preserved) until the caller commits and drains.
    #[tokio::test]
    async fn deferred_create_preserves_payload_and_fires_after_commit() {
        let state = crate::utils::test_utils::get_app_state().await;

        let seeded: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'alcedocore_roles')",
        )
        .bind(SCHEMA)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap_or(false);
        if !seeded {
            eprintln!("skipping: {SCHEMA} not seeded");
            return;
        }

        type Captured = Vec<(Vec<Map<String, Value>>, String)>;
        let captured: Arc<Mutex<Captured>> = Arc::default();
        let sink = captured.clone();
        state
            .event_bus
            .on::<ItemsAfterCreate, _>(
                "after.items.create.alcedocore_roles",
                move |event, _context, _state, _tx| {
                    let sink = sink.clone();
                    Box::pin(async move {
                        sink.lock()
                            .unwrap()
                            .push((event.items.clone(), event.collection.clone()));
                    })
                },
            )
            .await;

        let role_id = Uuid::new_v4();
        let collection = "alcedocore_roles".to_string();
        let ctx = ctx();
        let mut service = ItemsService::new(&state, &ctx, &collection);

        let mut tx = state.database_pool.begin().await.unwrap();
        let item = item_map! {
            "id" => role_id.to_string(),
            "name" => format!("after-commit-{role_id}"),
            "description" => "",
            "is_system" => false,
        };
        service
            .create_many(vec![item], &mut Some(&mut tx))
            .await
            .unwrap();

        assert!(
            captured.lock().unwrap().is_empty(),
            "the deferred hook must not fire before the caller commits"
        );

        tx.commit().await.unwrap();
        service.run_after_commit_sync().await;

        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 1, "the deferred hook fires exactly once");
        let (items, collection) = &captured[0];
        assert_eq!(collection, "alcedocore_roles");
        assert_eq!(items.len(), 1, "the created row is preserved in the event");
        assert_eq!(
            items[0].get("id").and_then(Value::as_str),
            Some(role_id.to_string().as_str()),
            "the event carries the created row (with its generated pk)"
        );
        assert_eq!(
            items[0].get("name").and_then(Value::as_str),
            Some(format!("after-commit-{role_id}").as_str())
        );
        drop(captured);

        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_roles WHERE id = $1"
        ))
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();
    }

    /// Dropping a `TxGuard` without committing rolls the transaction back and
    /// discards the events it held — nothing fires and nothing is written.
    #[tokio::test]
    async fn dropped_guard_discards_events_and_rolls_back() {
        let state = crate::utils::test_utils::get_app_state().await;

        let seeded: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'alcedocore_roles')",
        )
        .bind(SCHEMA)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap_or(false);
        if !seeded {
            eprintln!("skipping: {SCHEMA} not seeded");
            return;
        }

        type Captured = Vec<(Vec<Map<String, Value>>, String)>;
        let captured: Arc<Mutex<Captured>> = Arc::default();
        let sink = captured.clone();
        state
            .event_bus
            .on::<ItemsAfterCreate, _>(
                "after.items.create.alcedocore_roles",
                move |event, _context, _state, _tx| {
                    let sink = sink.clone();
                    Box::pin(async move {
                        sink.lock()
                            .unwrap()
                            .push((event.items.clone(), event.collection.clone()));
                    })
                },
            )
            .await;

        let role_id = Uuid::new_v4();
        let collection = "alcedocore_roles".to_string();
        let ctx = ctx();
        let mut service = ItemsService::new(&state, &ctx, &collection);

        {
            let mut guard = TxGuard::new(state.database_pool.begin().await.unwrap());
            let item = item_map! {
                "id" => role_id.to_string(),
                "name" => format!("dropped-{role_id}"),
                "description" => "",
                "is_system" => false,
            };
            service
                .create_many(vec![item], &mut Some(guard.tx()))
                .await
                .unwrap();
            guard.append(service.take_pending_after());
            // `guard` is dropped here without `commit`.
        }

        assert!(
            captured.lock().unwrap().is_empty(),
            "a dropped guard must not dispatch its deferred events"
        );

        let exists: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM \"{SCHEMA}\".alcedocore_roles WHERE id = $1)"
        ))
        .bind(role_id)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert!(!exists, "a dropped guard must roll the write back");
    }
}
