use std::sync::Arc;

use serde_json::{Map, Value};

use crate::services::{
    activity_logs::{self, ActivityLogEntry},
    hooks::{
        MultiEventBus,
        types::{
            items_create::ItemsAfterCreate, items_delete::ItemsAfterDelete,
            items_update::ItemsAfterUpdate,
        },
    },
};

/// Registers system hooks that write activity-log rows for item CRUD events.
/// Entries go through the async channel (`state.activity_log_tx`), never
/// directly to the DB, so the request path never waits for log writes.
pub async fn setup_activity_log_hooks(bus: &Arc<MultiEventBus>) {
    bus.on::<ItemsAfterCreate, _>("after.items.create", |event, context, state, _tx| {
        Box::pin(async move {
            let action = activity_logs::action_for(&event.collection, "created");
            let collection_name = if activity_logs::is_system_collection_name(&event.collection)
            {
                None
            } else {
                Some(event.collection.clone())
            };
            for item in &event.items {
                let item_id = item.get(&event.pk_name).cloned();
                state.activity_log_tx.try_send(ActivityLogEntry {
                    context: context.clone(),
                    action: action.clone(),
                    target: event.collection.clone(),
                    description: None,
                    metadata: Value::Object(item.clone()),
                    diff: None,
                    collection_name: collection_name.clone(),
                    item_id,
                });
            }
        })
    })
    .await;

    bus.on::<ItemsAfterUpdate, _>("after.items.update", |event, context, state, _tx| {
        Box::pin(async move {
            let action = activity_logs::action_for(&event.collection, "updated");
            let collection_name = if activity_logs::is_system_collection_name(&event.collection)
            {
                None
            } else {
                Some(event.collection.clone())
            };
            for (i, old_item) in event.old_items.iter().enumerate() {
                let item_id = event
                    .keys
                    .get(i)
                    .map(|k| Value::String(k.clone()))
                    .or_else(|| old_item.get("id").cloned());
                let diff = build_diff(old_item, &event.payload);
                state.activity_log_tx.try_send(ActivityLogEntry {
                    context: context.clone(),
                    action: action.clone(),
                    target: event.collection.clone(),
                    description: None,
                    metadata: Value::Object(old_item.clone()),
                    diff: Some(diff),
                    collection_name: collection_name.clone(),
                    item_id,
                });
            }
        })
    })
    .await;

    bus.on::<ItemsAfterDelete, _>("after.items.delete", |event, context, state, _tx| {
        Box::pin(async move {
            let action = activity_logs::action_for(&event.collection, "deleted");
            let collection_name = if activity_logs::is_system_collection_name(&event.collection)
            {
                None
            } else {
                Some(event.collection.clone())
            };
            for (i, key) in event.keys.iter().enumerate() {
                let old_row = event
                    .old_rows
                    .iter()
                    .find(|row| row.values().any(|v| v == key))
                    .or_else(|| event.old_rows.get(i));
                let metadata = old_row
                    .map(|row| Value::Object(row.clone()))
                    .unwrap_or(Value::Null);
                let diff = old_row.map(build_delete_diff);
                state.activity_log_tx.try_send(ActivityLogEntry {
                    context: context.clone(),
                    action: action.clone(),
                    target: event.collection.clone(),
                    description: None,
                    metadata,
                    diff,
                    collection_name: collection_name.clone(),
                    item_id: Some(key.clone()),
                });
            }
        })
    })
    .await;
}

/// Builds a field-level diff between an old row and the update payload:
/// `{ "<field>": { "old": <old>, "new": <payload> } }` for every payload field
/// whose value actually changed.
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::Mutex;
    use uuid::Uuid;

    use super::*;
    use crate::item_map;
    use crate::services::{
        activity_logs::{spawn_log_writer, ActivityLogChannel},
        context::{AppContext, RequestSource},
        items::{query::Query, service::ItemsService},
    };

    const SCHEMA: &str = "testing010production";

    fn app_ctx() -> AppContext {
        AppContext {
            app_name: "testing".to_string(),
            version: "production".to_string(),
            // SystemTest bypasses policy checks so the test can write to a
            // user collection without seeding roles; it is still logged.
            request_source: RequestSource::SystemTest,
            identity: None,
            request_id: None,
        }
    }

    async fn schema_seeded(state: &crate::AppState) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'cases')",
        )
        .bind(SCHEMA)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap_or(false)
    }

    /// Polls `sql` (a SELECT returning bool) until true or ~5s elapse.
    async fn wait_for(state: &crate::AppState, mut make_sql: impl FnMut() -> String) -> bool {
        for _ in 0..100 {
            let sql = make_sql();
            let found: bool = sqlx::query_scalar(&sql)
                .fetch_one(&*state.database_pool)
                .await
                .unwrap_or(false);
            if found {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    /// End-to-end: item CRUD must land in the log tables of the schema the
    /// write happened in — user collections in `{schema}.alcedocore_collection_logs`,
    /// system collections in `{schema}.alcedocore_system_logs`, and global
    /// (system-context) writes in `alcedocore.alcedocore_system_logs`.
    #[tokio::test]
    async fn crud_logs_route_to_the_right_schema_and_table() {
        let mut state = crate::utils::test_utils::get_app_state().await;
        if !schema_seeded(&state).await {
            eprintln!("skipping: {SCHEMA} not seeded");
            return;
        }

        // The shared test state forgets its channel receiver, so wire a real
        // channel + background writer for this test.
        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        let spill = std::fs::File::create("/dev/null").unwrap();
        state.activity_log_tx = ActivityLogChannel::new(tx, Arc::new(Mutex::new(spill)));
        spawn_log_writer(state.clone(), rx, 100, Duration::from_millis(50));
        setup_activity_log_hooks(&state.event_bus).await;

        let ctx = app_ctx();

        // create in a user collection -> {schema}.alcedocore_collection_logs
        let case_id = Uuid::new_v4();
        let id_str = case_id.to_string();
        let cases_collection = "cases".to_string();
        let mut cases = ItemsService::new(&state, &ctx, &cases_collection);
        cases
            .create_many(
                vec![item_map! {
                    "id" => id_str.clone(),
                    "title" => "Log Test",
                    "state" => "open",
                }],
                &mut None,
            )
            .await
            .unwrap();
        assert!(
            wait_for(&state, || format!(
                "SELECT EXISTS(SELECT 1 FROM \"{SCHEMA}\".alcedocore_collection_logs \
                 WHERE collection_name = 'cases' AND action = 'item_created' \
                 AND item_id::text LIKE '%{id_str}%')"
            ))
            .await,
            "create on cases must log to {SCHEMA}.alcedocore_collection_logs"
        );

        // update -> item_updated with a field diff
        let mut update = Map::new();
        update.insert(
            "title".to_string(),
            Value::String("Log Test Updated".to_string()),
        );
        cases
            .update_items_by_query(
                &mut Query::eq("id", Value::String(id_str.clone())),
                update,
                &mut None,
            )
            .await
            .unwrap();
        assert!(
            wait_for(&state, || format!(
                "SELECT EXISTS(SELECT 1 FROM \"{SCHEMA}\".alcedocore_collection_logs \
                 WHERE collection_name = 'cases' AND action = 'item_updated' \
                 AND item_id::text LIKE '%{id_str}%' AND diff ? 'title')"
            ))
            .await,
            "update on cases must log item_updated with a diff"
        );

        // delete -> item_deleted
        cases
            .delete_items_by_pks(vec![Value::String(id_str.clone())], None)
            .await
            .unwrap();
        assert!(
            wait_for(&state, || format!(
                "SELECT EXISTS(SELECT 1 FROM \"{SCHEMA}\".alcedocore_collection_logs \
                 WHERE collection_name = 'cases' AND action = 'item_deleted' \
                 AND item_id::text LIKE '%{id_str}%')"
            ))
            .await,
            "delete on cases must log item_deleted"
        );

        // system collection in an app schema -> {schema}.alcedocore_system_logs
        let role_id = Uuid::new_v4();
        let role_name = format!("log-{role_id}");
        let roles_collection = "alcedocore_roles".to_string();
        let mut roles = ItemsService::new(&state, &ctx, &roles_collection);
        roles
            .create_many(
                vec![item_map! {
                    "id" => role_id.to_string(),
                    "name" => role_name.clone(),
                    "description" => "",
                    "is_system" => false,
                }],
                &mut None,
            )
            .await
            .unwrap();
        assert!(
            wait_for(&state, || format!(
                "SELECT EXISTS(SELECT 1 FROM \"{SCHEMA}\".alcedocore_system_logs \
                 WHERE target = 'alcedocore_roles' AND action = 'role_created' \
                 AND metadata->>'name' = '{role_name}')"
            ))
            .await,
            "role write must log to {SCHEMA}.alcedocore_system_logs"
        );

        // global write (system context) -> alcedocore.alcedocore_system_logs
        let user_id = Uuid::new_v4();
        let email = format!("log-{user_id}@test.local");
        let system_ctx = AppContext::system(RequestSource::API);
        let users_collection = "alcedocore_users".to_string();
        let mut users = ItemsService::new(&state, &system_ctx, &users_collection);
        users
            .create_many(
                vec![item_map! {
                    "id" => user_id.to_string(),
                    "email" => email.clone(),
                    "password_hash" => "secret-hash",
                    "is_admin" => false,
                }],
                &mut None,
            )
            .await
            .unwrap();
        assert!(
            wait_for(&state, || format!(
                "SELECT EXISTS(SELECT 1 FROM alcedocore.alcedocore_system_logs \
                 WHERE target = 'alcedocore_users' AND action = 'user_created' \
                 AND metadata->>'email' = '{email}')"
            ))
            .await,
            "user write must log to alcedocore.alcedocore_system_logs"
        );

        // the credential field must be redacted from the audit metadata
        let leaked: Option<String> = sqlx::query_scalar(&format!(
            "SELECT metadata->>'password_hash' FROM alcedocore.alcedocore_system_logs \
             WHERE target = 'alcedocore_users' AND metadata->>'email' = '{email}'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert!(leaked.is_none(), "password_hash must be redacted from logs");

        // cleanup
        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_collection_logs \
             WHERE collection_name = 'cases' AND item_id::text LIKE '%{id_str}%'"
        ))
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_system_logs \
             WHERE target = 'alcedocore_roles' AND metadata->>'name' = '{role_name}'"
        ))
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "DELETE FROM alcedocore.alcedocore_system_logs \
             WHERE target = 'alcedocore_users' AND metadata->>'email' = '{email}'"
        ))
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_roles WHERE id = $1"
        ))
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM alcedocore.alcedocore_users WHERE id = $1")
            .bind(user_id)
            .execute(&*state.database_pool)
            .await
            .unwrap();
    }
}
