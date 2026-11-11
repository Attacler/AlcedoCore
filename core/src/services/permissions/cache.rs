//! Caching for permission resolution.
//!
//! Resolving access is a pure read of `alcedocore_user_roles` (which roles an
//! identity holds) composed with `alcedocore_policy_permissions` joined to
//! `alcedocore_role_policies` (what those roles grant). Both are written only
//! through `ItemsService`, so the system-hook bus can invalidate them.
//!
//! Two layers:
//!
//! - `auth:sessions:{schema}:{userId}` → the caller's role ids, plus whether they
//!   are an app admin. TTL is short (the session's own lifetime is enforced
//!   elsewhere); the `alcedocore_user_roles` / `alcedocore_role_scopes` hooks refresh it.
//! - `auth:rules:{schema}:{roleId}` → every policy permission attached to that
//!   role, as the raw DB rows. TTL is long (24h) because the policy hooks
//!   refresh it on any write.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::AppState;
use crate::services::errors::AlcedoError;

/// Long TTL for the rule layer; the policy hooks are the primary invalidation.
const RULES_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Short TTL for the caller layer. Role membership changes are hook-invalidated;
/// this is only a backstop against a missed invalidate.
const SESSIONS_TTL: Duration = Duration::from_secs(60 * 60);

/// The `public` role has no user, so it gets a sentinel key.
pub const PUBLIC_IDENTITY: &str = "public";

fn user_roles_key(schema: &str, user_id: &str) -> String {
    format!("auth:sessions:{schema}:{user_id}")
}

fn role_rules_key(schema: &str, role_id: &str) -> String {
    format!("auth:rules:{schema}:{role_id}")
}

/// A caller's cached role membership, plus the `is_app_admin` verdict that is
/// derived from the same tables (`alcedocore_roles` + `alcedocore_role_scopes`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedIdentity {
    pub role_ids: Vec<String>,
    pub is_app_admin: bool,
    pub is_admin: bool,
}

/// One raw `alcedocore_policy_permissions` row, as resolved for a role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPermission {
    pub collection: i32,
    pub action: String,
    pub fields: Value,
    pub filter: Value,
    pub field_validation: Value,
}

// ---------------------------------------------------------------------------
// Caller layer
// ---------------------------------------------------------------------------

/// Resolves (and caches) the caller's roles and app-admin verdict.
pub async fn cached_identity(
    state: &AppState,
    schema: &str,
    user_id: Uuid,
) -> Result<CachedIdentity, AlcedoError> {
    let key = user_roles_key(schema, &user_id.to_string());
    if let Ok(Some(raw)) = state.cache.get(&key).await {
        if let Ok(cached) = serde_json::from_str::<CachedIdentity>(&raw) {
            return Ok(cached);
        }
    }

    let identity = load_identity(state, schema, user_id).await?;
    if let Ok(raw) = serde_json::to_string(&identity) {
        let _ = state.cache.set_ttl(key, raw, SESSIONS_TTL).await;
    }
    Ok(identity)
}

/// The `public` identity: the seeded `public` role, never an admin. Cached
/// under a sentinel key so anonymous callers share one lookup.
pub async fn cached_public_identity(
    state: &AppState,
    schema: &str,
) -> Result<CachedIdentity, AlcedoError> {
    let key = user_roles_key(schema, PUBLIC_IDENTITY);
    if let Ok(Some(raw)) = state.cache.get(&key).await {
        if let Ok(cached) = serde_json::from_str::<CachedIdentity>(&raw) {
            return Ok(cached);
        }
    }

    let identity = load_public_identity(state, schema).await?;
    if let Ok(raw) = serde_json::to_string(&identity) {
        let _ = state.cache.set_ttl(key, raw, SESSIONS_TTL).await;
    }
    Ok(identity)
}

/// Drops a caller's cached roles. Called by the `alcedocore_user_roles` /
/// `alcedocore_role_scopes` hooks; without a user id (a role-wide change) every
/// caller layer for the schema is dropped.
pub async fn invalidate_identity(state: &AppState, schema: &str, user_id: Option<Uuid>) {
    match user_id {
        Some(user_id) => {
            let _ = state
                .cache
                .del(&user_roles_key(schema, &user_id.to_string()))
                .await;
        }
        None => invalidate_pattern(state, &format!("auth:sessions:{schema}:")).await,
    }
}

// ---------------------------------------------------------------------------
// Rule layer
// ---------------------------------------------------------------------------

/// Resolves (and caches) every policy permission attached to `role_id`.
pub async fn cached_role_permissions(
    state: &AppState,
    schema: &str,
    role_id: &str,
) -> Result<Vec<CachedPermission>, AlcedoError> {
    let key = role_rules_key(schema, role_id);
    if let Ok(Some(raw)) = state.cache.get(&key).await {
        if let Ok(cached) = serde_json::from_str::<Vec<CachedPermission>>(&raw) {
            return Ok(cached);
        }
    }

    let permissions = load_role_permissions(state, schema, role_id).await?;
    if let Ok(raw) = serde_json::to_string(&permissions) {
        let _ = state.cache.set_ttl(key, raw, RULES_TTL).await;
    }
    Ok(permissions)
}

/// Drops every cached entry for `schema`. The blunt instrument used when a
/// change cannot be attributed to specific roles.
pub async fn invalidate_schema(state: &AppState, schema: &str) {
    invalidate_pattern(state, &format!("auth:rules:{schema}:")).await;
    invalidate_pattern(state, &format!("auth:sessions:{schema}:")).await;
}

async fn invalidate_pattern(state: &AppState, prefix: &str) {
    if let Ok(keys) = state.cache.get_keys(&format!("{prefix}*")).await {
        for key in keys {
            let _ = state.cache.del(&key).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Loaders (the uncached path)
// ---------------------------------------------------------------------------

async fn load_identity(
    state: &AppState,
    schema: &str,
    user_id: Uuid,
) -> Result<CachedIdentity, AlcedoError> {
    use crate::services::postgres::pool::{execute_query, pgrow_to_json};
    use sea_query::{Alias, Expr, PostgresQueryBuilder};

    let sql = sea_query::Query::select()
        .column(Alias::new("role_id"))
        .from((Alias::new(schema), Alias::new("alcedocore_user_roles")))
        .and_where(Expr::col(Alias::new("user_id")).eq(Expr::value(user_id.to_string())))
        .to_string(PostgresQueryBuilder);

    let rows = execute_query(state, sql).await?;
    let mut role_ids = Vec::with_capacity(rows.len());
    for row in &rows {
        let map = pgrow_to_json(row)?;
        if let Some(role_id) = map.get("role_id").and_then(Value::as_str) {
            role_ids.push(role_id.to_string());
        }
    }

    let is_app_admin =
        crate::services::permissions::read::is_app_admin_in_schema(state, schema, user_id).await?;
    let is_admin = crate::services::permissions::read::is_admin_user(state, user_id).await?;

    Ok(CachedIdentity {
        role_ids,
        is_app_admin,
        is_admin,
    })
}

async fn load_public_identity(
    state: &AppState,
    schema: &str,
) -> Result<CachedIdentity, AlcedoError> {
    use crate::services::postgres::pool::{execute_query, pgrow_to_json};
    use sea_query::{Alias, Expr, PostgresQueryBuilder};

    let sql = sea_query::Query::select()
        .column(Alias::new("id"))
        .from((Alias::new(schema), Alias::new("alcedocore_roles")))
        .and_where(Expr::col(Alias::new("name")).eq(Expr::value("public")))
        .to_string(PostgresQueryBuilder);

    let rows = execute_query(state, sql).await?;
    let mut role_ids = Vec::with_capacity(rows.len());
    for row in &rows {
        let map = pgrow_to_json(row)?;
        if let Some(id) = map.get("id").and_then(Value::as_str) {
            role_ids.push(id.to_string());
        }
    }

    Ok(CachedIdentity {
        role_ids,
        is_app_admin: false,
        is_admin: false,
    })
}

async fn load_role_permissions(
    state: &AppState,
    schema: &str,
    role_id: &str,
) -> Result<Vec<CachedPermission>, AlcedoError> {
    use crate::services::postgres::pool::{execute_query, pgrow_to_json};
    use sea_query::{Alias, Expr, JoinType, PostgresQueryBuilder};

    let mut select = sea_query::Query::select();
    select
        .from_as(
            (Alias::new(schema), Alias::new("alcedocore_role_policies")),
            Alias::new("rp"),
        )
        .join_as(
            JoinType::InnerJoin,
            (
                Alias::new(schema),
                Alias::new("alcedocore_policy_permissions"),
            ),
            Alias::new("pp"),
            Expr::col((Alias::new("pp"), Alias::new("policy_id")))
                .equals((Alias::new("rp"), Alias::new("policy_id"))),
        )
        .column((Alias::new("pp"), Alias::new("collection")))
        .column((Alias::new("pp"), Alias::new("action")))
        .column((Alias::new("pp"), Alias::new("fields")))
        .column((Alias::new("pp"), Alias::new("filter")))
        .column((Alias::new("pp"), Alias::new("field_validation")))
        .and_where(Expr::col((Alias::new("rp"), Alias::new("role_id"))).eq(Expr::value(role_id)));

    let sql = select.to_string(PostgresQueryBuilder);
    let rows = execute_query(state, sql).await?;

    let mut permissions = Vec::with_capacity(rows.len());
    for row in &rows {
        let map = pgrow_to_json(row)?;
        let Some(collection) = map.get("collection").and_then(Value::as_i64) else {
            continue;
        };
        let Some(action) = map.get("action").and_then(Value::as_str) else {
            continue;
        };
        permissions.push(CachedPermission {
            collection: collection as i32,
            action: action.to_string(),
            fields: map.get("fields").cloned().unwrap_or(Value::Null),
            filter: map.get("filter").cloned().unwrap_or(Value::Null),
            field_validation: map.get("field_validation").cloned().unwrap_or(Value::Null),
        });
    }

    Ok(permissions)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = "crm010production";

    /// Exercises both cache layers end to end: load, cache-hit (a direct DB
    /// write is not seen while warm), and invalidation (schema purge and
    /// caller purge force a reload).
    #[tokio::test]
    async fn cache_layers_load_hit_and_invalidate() {
        let state = crate::utils::test_utils::get_app_state().await;

        let schema_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'alcedocore_roles')",
        )
        .bind(SCHEMA)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap_or(false);
        if !schema_exists {
            eprintln!("skipping: {SCHEMA} not seeded");
            return;
        }

        let user_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();
        let policy_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO alcedocore.alcedocore_users (id, email, password_hash, is_admin) \
             VALUES ($1, $2, 'x', false)",
        )
        .bind(user_id)
        .bind(format!("cache-perm-{user_id}@test.local"))
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{SCHEMA}\".alcedocore_roles (id, name, description, is_system) \
             VALUES ($1, $2, '', false)"
        ))
        .bind(role_id)
        .bind(format!("cache-perm-{role_id}"))
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{SCHEMA}\".alcedocore_user_roles (id, user_id, role_id) VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{SCHEMA}\".alcedocore_policies (id, name, description) VALUES ($1, $2, '')"
        ))
        .bind(policy_id)
        .bind(format!("cache-perm-{policy_id}"))
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{SCHEMA}\".alcedocore_role_policies (id, role_id, policy_id) \
             VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(role_id)
        .bind(policy_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();

        async fn insert_permission(state: &AppState, policy_id: Uuid, permission_id: Uuid) {
            sqlx::query(&format!(
                "INSERT INTO \"{SCHEMA}\".alcedocore_policy_permissions \
                 (id, policy_id, collection, action, fields, filter, field_validation) \
                 VALUES ($1, $2, 1, 'read', '[]'::json, '[]'::json, '[]'::json)"
            ))
            .bind(permission_id)
            .bind(policy_id)
            .execute(&*state.database_pool)
            .await
            .unwrap();
        }

        insert_permission(&state, policy_id, Uuid::new_v4()).await;

        // Layer 1: the caller's roles resolve from the DB.
        let identity = cached_identity(&state, SCHEMA, user_id).await.unwrap();
        assert_eq!(identity.role_ids, vec![role_id.to_string()]);
        assert!(!identity.is_app_admin);
        assert!(!identity.is_admin, "seeded user is not a global admin");

        let sessions_key = format!("auth:sessions:{SCHEMA}:{user_id}");
        assert!(
            state.cache.get(&sessions_key).await.unwrap().is_some(),
            "identity layer must be cached"
        );

        // Layer 2: the role's permissions resolve from the DB.
        let role_key = role_id.to_string();
        assert_eq!(
            cached_role_permissions(&state, SCHEMA, &role_key)
                .await
                .unwrap()
                .len(),
            1
        );

        // A direct DB write (no `ItemsService` hook) is invisible while warm,
        // proving the second read is served from the cache.
        insert_permission(&state, policy_id, Uuid::new_v4()).await;
        assert_eq!(
            cached_role_permissions(&state, SCHEMA, &role_key)
                .await
                .unwrap()
                .len(),
            1,
            "warm cache must not re-read the DB"
        );

        // A schema purge drops both layers, so the next read sees both rows.
        invalidate_schema(&state, SCHEMA).await;
        assert_eq!(
            cached_role_permissions(&state, SCHEMA, &role_key)
                .await
                .unwrap()
                .len(),
            2,
            "invalidation must force a reload"
        );
        assert!(
            state.cache.get(&sessions_key).await.unwrap().is_none(),
            "schema purge also drops the caller layer"
        );

        // The caller layer can be purged on its own.
        cached_identity(&state, SCHEMA, user_id).await.unwrap();
        invalidate_identity(&state, SCHEMA, None).await;
        assert!(
            state.cache.get(&sessions_key).await.unwrap().is_none(),
            "identity purge drops the caller layer"
        );

        // Cleanup.
        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_policy_permissions WHERE policy_id = $1"
        ))
        .bind(policy_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_role_policies WHERE role_id = $1"
        ))
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "DELETE FROM \"{SCHEMA}\".alcedocore_policies WHERE id = $1"
        ))
        .bind(policy_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();
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
}
