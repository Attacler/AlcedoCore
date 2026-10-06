use futures::future::join_all;
use sea_query::{Alias, Expr, PostgresQueryBuilder, SimpleExpr};
use serde_json::{Map, Value};
use sqlx::{Postgres, Row, Transaction};
use std::collections::{HashMap, HashSet};

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
use crate::{AppState, services::activity_logs, services::errors::AlcedoError};
use tokio::sync::OnceCell;

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
    /// A deferred activity-log row, written at the same post-commit boundary as
    /// the `after` hooks so it observes (and never precedes) the committed
    /// write. Carries its own context like the hook variants.
    Log { entry: ActivityLogEntry },
}

/// One pending activity-log insert. Built while the write data is still in
/// hand, then flushed by [`AfterCommitQueue::run`].
pub struct ActivityLogEntry {
    pub context: AppContext,
    pub action: String,
    pub target: String,
    pub description: Option<String>,
    pub metadata: Value,
    pub diff: Option<Value>,
    pub collection_name: Option<String>,
    pub item_id: Option<Value>,
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
                        context,
                        state: state.clone(),
                        tx: None,
                    };
                    state
                        .event_bus
                        .trigger(&key, &mut event, hook_context)
                        .await;
                }
                PendingAfter::Update {
                    key,
                    context,
                    mut event,
                } => {
                    let hook_context = HookContext {
                        context,
                        state: state.clone(),
                        tx: None,
                    };
                    state
                        .event_bus
                        .trigger(&key, &mut event, hook_context)
                        .await;
                }
                PendingAfter::Delete {
                    key,
                    context,
                    mut event,
                } => {
                    let hook_context = HookContext {
                        context,
                        state: state.clone(),
                        tx: None,
                    };
                    state
                        .event_bus
                        .trigger(&key, &mut event, hook_context)
                        .await;
                }
                PendingAfter::Log { entry } => {
                    if let Err(err) = activity_logs::record(
                        state,
                        &entry.context,
                        &entry.action,
                        &entry.target,
                        entry.description.clone(),
                        entry.metadata.clone(),
                        entry.diff.clone(),
                        entry.collection_name.as_deref(),
                        entry.item_id.clone(),
                        None,
                    )
                    .await
                    {
                        // Verification/audit failures must never fail the write
                        // they describe; surface the problem and move on.
                        eprintln!("activity log write failed: {}", err);
                    }
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
}

impl<'c> TxGuard<'c> {
    pub fn new(tx: Transaction<'c, Postgres>) -> Self {
        Self {
            tx,
            after: AfterCommitQueue::default(),
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

    /// Commits, then dispatches the deferred `after` hooks. Consumes the guard,
    /// so the events fire at most once.
    pub async fn commit(self, state: &AppState) -> Result<(), AlcedoError> {
        let TxGuard { tx, mut after } = self;
        tx.commit().await?;
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

/// Builds a field-level diff between an old row and the update payload:
/// `{ "<field>": { "old": <old>, "new": <payload> } }` for every payload field
/// whose value actually changed. An empty object means nothing changed.
fn build_diff(old_item: &Map<String, Value>, payload: &Map<String, Value>) -> Value {
    let mut diff = Map::new();
    for (field, new) in payload {
        let changed = old_item.get(field) != Some(new);
        if changed {
            let mut change = Map::new();
            change.insert(
                "old".to_string(),
                old_item.get(field).cloned().unwrap_or(Value::Null),
            );
            change.insert("new".to_string(), new.clone());
            diff.insert(field.clone(), Value::Object(change));
        }
    }
    Value::Object(diff)
}

/// Builds the diff for a deleted item: every field's prior value, with `new`
/// set to `null` since the row no longer exists.
fn build_delete_diff(old_item: &Map<String, Value>) -> Value {
    let mut diff = Map::new();
    for (field, old) in old_item {
        let mut change = Map::new();
        change.insert("old".to_string(), old.clone());
        change.insert("new".to_string(), Value::Null);
        diff.insert(field.clone(), Value::Object(change));
    }
    Value::Object(diff)
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
    /// write. The caller must call this **after committing its transaction**,
    /// so the hooks observe the committed rows.
    pub async fn run_after_commit(&mut self) {
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

    /// Defers an `after.items.create.<collection>` hook to the commit boundary,
    /// plus one activity-log row per created item.
    ///
    /// `pk_name` is the primary-key column: `create_items_with_tx` injects it
    /// into each item, so the log can carry the item id.
    fn defer_after_create(&mut self, items: Vec<Map<String, Value>>, pk_name: String) {
        let action = activity_logs::action_for(self.collection, "created");
        let collection_name = self.log_collection_name();
        for item in &items {
            let item_id = item.get(&pk_name).cloned();
            self.pending_after.push(PendingAfter::Log {
                entry: ActivityLogEntry {
                    context: self.app_context.clone(),
                    action: action.clone(),
                    target: self.collection.clone(),
                    description: None,
                    metadata: Value::Object(item.clone()),
                    diff: None,
                    collection_name: collection_name.clone(),
                    item_id,
                },
            });
        }
        self.pending_after.push(PendingAfter::Create {
            key: format!("after.items.create.{}", self.collection),
            context: self.app_context.clone(),
            event: ItemsAfterCreate {
                items,
                collection: self.collection.clone(),
            },
        });
    }

    /// Defers an `after.items.update.<collection>` hook to the commit boundary,
    /// plus one activity-log row per updated item carrying a field-level diff.
    fn defer_after_update(
        &mut self,
        keys: Vec<String>,
        payload: Map<String, Value>,
        old_items: Vec<Map<String, Value>>,
        pk_name: String,
    ) {
        let action = activity_logs::action_for(self.collection, "updated");
        let collection_name = self.log_collection_name();
        for item in &old_items {
            let item_id = item.get(&pk_name).cloned();
            let diff = build_diff(item, &payload);
            self.pending_after.push(PendingAfter::Log {
                entry: ActivityLogEntry {
                    context: self.app_context.clone(),
                    action: action.clone(),
                    target: self.collection.clone(),
                    description: None,
                    metadata: Value::Object(item.clone()),
                    diff: Some(diff),
                    collection_name: collection_name.clone(),
                    item_id,
                },
            });
        }
        self.pending_after.push(PendingAfter::Update {
            key: format!("after.items.update.{}", self.collection),
            context: self.app_context.clone(),
            event: ItemsAfterUpdate {
                keys,
                payload,
                collection: self.collection.clone(),
            },
        });
    }

    /// Defers an `after.items.delete.<collection>` hook to the commit boundary,
    /// plus one activity-log row per deleted item carrying the item's values as
    /// a `{ field: { old: <value>, new: null } }` diff.
    fn defer_after_delete(&mut self, keys: Vec<Value>, old_rows: Vec<Map<String, Value>>) {
        let action = activity_logs::action_for(self.collection, "deleted");
        let collection_name = self.log_collection_name();
        for key in &keys {
            // Match the old row by primary key when available; fall back to
            // whatever was read.
            let old_row = old_rows
                .iter()
                .find(|row| row.values().any(|v| v == key))
                .or_else(|| old_rows.first());
            let item_id = key.clone();
            let metadata = old_row
                .map(|row| Value::Object(row.clone()))
                .unwrap_or(Value::Null);
            let diff = old_row.map(build_delete_diff);
            self.pending_after.push(PendingAfter::Log {
                entry: ActivityLogEntry {
                    context: self.app_context.clone(),
                    action: action.clone(),
                    target: self.collection.clone(),
                    description: None,
                    metadata,
                    diff,
                    collection_name: collection_name.clone(),
                    item_id: Some(item_id),
                },
            });
        }
        self.pending_after.push(PendingAfter::Delete {
            key: format!("after.items.delete.{}", self.collection),
            context: self.app_context.clone(),
            event: ItemsAfterDelete {
                keys,
                collection: self.collection.clone(),
            },
        });
    }

    /// `Some(collection)` for a normal user collection (logged as a collection
    /// log), `None` for a system collection (logged as an app system log).
    fn log_collection_name(&self) -> Option<String> {
        if activity_logs::is_system_collection_name(self.collection) {
            None
        } else {
            Some(self.collection.clone())
        }
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
            let pk = pk_value_to_string(pk_data.get(&pk_name).unwrap())?;
            update_items.push(pk);
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

        let pk_name = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?
        .name;
        let pk_name_cloned = pk_name.clone();
        let mut created_items = vec![];
        for query in queries {
            let query = query.unwrap();
            let result = execute_query_transaction(&self.app_state, tx, &query).await?;
            let pk = pgrow_to_json(result.get(0).unwrap()).unwrap();
            let pk = pk_value_to_string(pk.get(&pk_name).unwrap())?;

            created_items.push(pk);
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

        Ok((total_deleted, before.keys, old_rows))
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
            let value = match parse_value(
                &schema,
                &self.app_context.schema_name(),
                &vec![],
                &self.collection,
                &field,
                value,
            ) {
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
        let pk = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?;
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
                            format!("Field {} is not nullable and received invalid input", field),
                            1,
                        ));
                    }
                }
                Some(val) => val,
            };

            values.push((Alias::new(field), value));
        }
        let pk = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?;
        stmt.returning_col(Alias::new(pk.name.clone()));
        stmt.values(values);
        let parsed_pk = self.parse_pk_value(&pk.name, &pk_val).await?;
        stmt.and_where(Expr::eq(Expr::col(Alias::new(pk.name.clone())), parsed_pk));

        Ok(stmt.to_string(PostgresQueryBuilder))
    }

    async fn generate_delete_item_query(&self, pk_val: &Value) -> Result<String, AlcedoError> {
        let mut stmt = sea_query::Query::delete();

        stmt.from_table((
            Alias::new(self.app_context.schema_name()),
            Alias::new(self.collection),
        ));

        let pk = get_pk_key(
            &self.app_state.database_schema,
            &self.app_context.schema_name(),
            &self.collection,
        )
        .await?;
        let parsed_pk = self.parse_pk_value(&pk.name, pk_val).await?;
        stmt.and_where(Expr::eq(Expr::col(Alias::new(pk.name.clone())), parsed_pk));
        Ok(stmt.to_string(PostgresQueryBuilder))
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
        service.run_after_commit().await;

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
