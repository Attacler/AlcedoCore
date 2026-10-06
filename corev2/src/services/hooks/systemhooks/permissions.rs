//! Invalidation hooks for the permission cache (`permissions::cache`).
//!
//! Resolving access reads two cached layers: the caller's roles (keyed by
//! schema + user) and the permissions attached to a role (keyed by schema +
//! role). Any write to the tables that feed them must drop the affected cached
//! entries, or a stale grant survives until its TTL.
//!
//! All of these tables are written through `ItemsService` (via `RolesService` /
//! `PoliciesService`), so the system-hook bus sees every change.
//!
//! Invalidation is deliberately schema-wide. A hook payload carries row keys,
//! not the `role_id` / `policy_id` needed to attribute a change to specific
//! cache entries (an update/delete gives no rows at all), and these writes are
//! rare, admin-only operations — so dropping the schema's entries is the cheap,
//! safe choice. Session entries (`auth:sessions:*`) are only dropped when the
//! write can change role membership or the app-admin verdict.

use std::sync::Arc;

use crate::AppState;
use crate::services::{
    hooks::{
        MultiEventBus,
        types::{
            items_create::ItemsAfterCreate, items_delete::ItemsAfterDelete,
            items_update::ItemsAfterUpdate,
        },
    },
    permissions::cache,
};

/// Policy/role tables whose writes change the resolved rules, and in the case
/// of `alcedocore_roles` / `alcedocore_role_scopes`, the cached app-admin verdict too.
const RULE_COLLECTIONS: &[&str] = &[
    "alcedocore_policy_permissions",
    "alcedocore_policies",
    "alcedocore_role_policies",
    "alcedocore_roles",
    "alcedocore_role_scopes",
];

/// `alcedocore_user_roles` feeds only the caller layer (role membership).
const USER_ROLES_COLLECTION: &str = "alcedocore_user_roles";

pub async fn setup_permission_hooks(bus: &Arc<MultiEventBus>) {
    for collection in RULE_COLLECTIONS {
        register(bus, collection, false).await;
    }
    register(bus, USER_ROLES_COLLECTION, true).await;
}

/// Registers the create/update/delete after-hooks for `collection`. When
/// `sessions_only` is set the purge is limited to the caller layer; otherwise
/// both layers for the schema are dropped.
async fn register(bus: &Arc<MultiEventBus>, collection: &str, sessions_only: bool) {
    bus.on::<ItemsAfterCreate, _>(
        &format!("after.items.create.{collection}"),
        move |_, context, state, _| {
            Box::pin(async move {
                invalidate(&state, &context.schema_name(), sessions_only).await;
            })
        },
    )
    .await;
    bus.on::<ItemsAfterUpdate, _>(
        &format!("after.items.update.{collection}"),
        move |_, context, state, _| {
            Box::pin(async move {
                invalidate(&state, &context.schema_name(), sessions_only).await;
            })
        },
    )
    .await;
    bus.on::<ItemsAfterDelete, _>(
        &format!("after.items.delete.{collection}"),
        move |_, context, state, _| {
            Box::pin(async move {
                invalidate(&state, &context.schema_name(), sessions_only).await;
            })
        },
    )
    .await;
}

async fn invalidate(state: &AppState, schema: &str, sessions_only: bool) {
    if sessions_only {
        // Without a user id this drops every caller entry for the schema; the
        // hook payload does not reliably carry the affected `user_id`.
        cache::invalidate_identity(state, schema, None).await;
    } else {
        cache::invalidate_schema(state, schema).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uuid::Uuid;

    use super::*;
    use crate::item_map;
    use crate::services::{
        context::{AppContext, RequestSource},
        items::service::ItemsService,
    };

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

    async fn schema_seeded(state: &AppState) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'alcedocore_roles')",
        )
        .bind(SCHEMA)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap_or(false)
    }

    async fn seed_keys(state: &AppState) -> (String, String) {
        let sessions = format!("auth:sessions:{SCHEMA}:probe");
        let rules = format!("auth:rules:{SCHEMA}:probe");
        let ttl = Duration::from_secs(60);
        state
            .cache
            .set_ttl(sessions.clone(), "[]".into(), ttl)
            .await
            .unwrap();
        state
            .cache
            .set_ttl(rules.clone(), "[]".into(), ttl)
            .await
            .unwrap();
        (sessions, rules)
    }

    async fn present(state: &AppState, key: &str) -> bool {
        state.cache.get(key).await.unwrap().is_some()
    }

    /// A write to a rule-table collection drops both cache layers for the
    /// schema (the collection name in the hook key must match the trigger).
    #[tokio::test]
    async fn role_write_purges_both_layers() {
        let state = crate::utils::test_utils::get_app_state().await;
        if !schema_seeded(&state).await {
            eprintln!("skipping: {SCHEMA} not seeded");
            return;
        }
        setup_permission_hooks(&state.event_bus).await;

        let role_id = Uuid::new_v4();
        let collection = "alcedocore_roles".to_string();
        let ctx = ctx();
        let mut service = ItemsService::new(&state, &ctx, &collection);

        let (sessions, rules) = seed_keys(&state).await;
        service
            .create_many(
                vec![item_map! {
                    "id" => role_id.to_string(),
                    "name" => format!("hook-{role_id}"),
                    "description" => "",
                    "is_system" => false,
                }],
                &mut None,
            )
            .await
            .unwrap();

        assert!(!present(&state, &sessions).await, "sessions must be purged");
        assert!(!present(&state, &rules).await, "rules must be purged");

        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_roles WHERE id = $1"
        ))
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();
    }

    /// A write to `alcedocore_user_roles` drops only the caller layer; the rule
    /// layer is untouched.
    #[tokio::test]
    async fn user_role_write_purges_only_sessions() {
        let state = crate::utils::test_utils::get_app_state().await;
        if !schema_seeded(&state).await {
            eprintln!("skipping: {SCHEMA} not seeded");
            return;
        }
        setup_permission_hooks(&state.event_bus).await;

        // Directly seed a user + role (no hooks) so the membership write has
        // valid foreign keys.
        let user_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO alcedocore.alcedocore_users (id, email, password_hash, is_admin) \
             VALUES ($1, $2, 'x', false)",
        )
        .bind(user_id)
        .bind(format!("hook-{user_id}@test.local"))
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{SCHEMA}\".alcedocore_roles (id, name, description, is_system) \
             VALUES ($1, $2, '', false)"
        ))
        .bind(role_id)
        .bind(format!("hook-{role_id}"))
        .execute(&*state.database_pool)
        .await
        .unwrap();

        let (sessions, rules) = seed_keys(&state).await;
        let link_id = Uuid::new_v4().to_string();
        let collection = "alcedocore_user_roles".to_string();
        let ctx = ctx();
        let mut service = ItemsService::new(&state, &ctx, &collection);
        service
            .create_many(
                vec![item_map! {
                    "id" => link_id,
                    "user_id" => user_id.to_string(),
                    "role_id" => role_id.to_string(),
                }],
                &mut None,
            )
            .await
            .unwrap();

        assert!(!present(&state, &sessions).await, "sessions must be purged");
        assert!(present(&state, &rules).await, "rules must be left intact");

        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_user_roles WHERE role_id = $1"
        ))
        .bind(role_id)
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

    /// An external-transaction write defers its `after` hook: the cache is
    /// untouched during the transaction, and only drained once the caller
    /// commits and calls `run_after_commit` — so the hook observes committed
    /// rows.
    #[tokio::test]
    async fn external_tx_write_drains_only_after_commit() {
        let state = crate::utils::test_utils::get_app_state().await;
        if !schema_seeded(&state).await {
            eprintln!("skipping: {SCHEMA} not seeded");
            return;
        }
        setup_permission_hooks(&state.event_bus).await;

        let (sessions, rules) = seed_keys(&state).await;
        let role_id = Uuid::new_v4();
        let collection = "alcedocore_roles".to_string();
        let ctx = ctx();
        let mut service = ItemsService::new(&state, &ctx, &collection);

        let mut tx = state.database_pool.begin().await.unwrap();
        service
            .create_many(
                vec![item_map! {
                    "id" => role_id.to_string(),
                    "name" => format!("hook-{role_id}"),
                    "description" => "",
                    "is_system" => false,
                }],
                &mut Some(&mut tx),
            )
            .await
            .unwrap();

        // Still inside the transaction: the hook must not have run yet.
        assert!(
            present(&state, &sessions).await && present(&state, &rules).await,
            "external-tx write must not purge before the caller commits"
        );

        tx.commit().await.unwrap();
        service.run_after_commit().await;
        assert!(!present(&state, &sessions).await, "sessions must be purged");
        assert!(!present(&state, &rules).await, "rules must be purged");

        sqlx::query(&format!("DELETE FROM \"{SCHEMA}\".alcedocore_roles WHERE id = $1"))
            .bind(role_id)
            .execute(&*state.database_pool)
            .await
            .unwrap();
    }
}
