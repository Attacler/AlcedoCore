use uuid::Uuid;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        auth::AuthService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        roles::RolesService,
    },
};

pub enum ScopeSource {
    User(Uuid),
    Public,
    Plugin(i64),
}

/// Does a granted scope cover a required one? Supports the v1 wildcards:
/// `kv.all` matches `kv.*` and `rootaccess.all` matches everything.
pub fn scope_matches(granted: &str, required: &str) -> bool {
    if granted == required {
        return true;
    }
    if granted == "rootaccess.all" {
        return true;
    }
    if let Some(prefix) = granted.strip_suffix(".all") {
        if required.starts_with(&format!("{}.", prefix)) {
            return true;
        }
    }
    false
}

async fn public_role_scopes(
    state: &AppState,
    ctx: &AppContext,
) -> Result<Vec<String>, AlcedoError> {
    match RolesService::new(state, ctx)
        .scopes_for_role_name("public")
        .await
    {
        Ok(scopes) => Ok(scopes),
        Err(e) => {
            tracing::warn!("[SCOPES] public role lookup failed, failing closed: {}", e);
            Ok(vec![])
        }
    }
}

async fn plugin_scopes(state: &AppState, install_id: i64) -> Result<Vec<String>, AlcedoError> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT granted_scopes::text
         FROM alcedocore.alcedocore_plugins_installs
         WHERE id = $1::int4",
    )
    .bind(install_id)
    .fetch_optional(state.database_pool.as_ref())
    .await?;

    let Some(raw) = raw else {
        tracing::warn!(
            "[SCOPES] install {} no longer exists, granting nothing",
            install_id
        );
        return Ok(vec![]);
    };

    match serde_json::from_str::<Vec<String>>(&raw) {
        Ok(scopes) => Ok(scopes),
        Err(e) => {
            tracing::warn!(
                "[SCOPES] install {} has unreadable granted_scopes: {}",
                install_id,
                e
            );
            Ok(vec![])
        }
    }
}

pub async fn check_entity_scope(
    state: &AppState,
    ctx: &AppContext,
    source: ScopeSource,
    required: &str,
) -> Result<(), AlcedoError> {
    let granted: Vec<String> = match source {
        ScopeSource::User(user_id) => {
            RolesService::new(state, ctx)
                .scopes_for_user(user_id)
                .await?
        }
        ScopeSource::Public => public_role_scopes(state, ctx).await?,
        ScopeSource::Plugin(install_id) => plugin_scopes(state, install_id).await?,
    };

    if granted.iter().any(|g| scope_matches(g, required)) {
        Ok(())
    } else {
        Err(AlcedoError::Forbidden(
            format!("Missing required scope: {}", required),
            0,
        ))
    }
}

pub async fn require_scope(
    state: &AppState,
    auth_level: &AuthLevel,
    ctx: &AppContext,
    required: &str,
) -> Result<(), AlcedoError> {
    match auth_level {
        AuthLevel::DeveloperKey { .. } => Ok(()),
        AuthLevel::User(user_id) => {
            let admin_ctx = AppContext::system(RequestSource::API);
            if AuthService::new(state, &admin_ctx)
                .is_admin(*user_id)
                .await?
            {
                return Ok(());
            }
            check_entity_scope(state, ctx, ScopeSource::User(*user_id), required).await
        }
        AuthLevel::Plugin(identity) => match identity.install_id {
            Some(install_id) => {
                check_entity_scope(state, ctx, ScopeSource::Plugin(install_id), required).await
            }
            None => check_entity_scope(state, ctx, ScopeSource::Public, required).await,
        },
        AuthLevel::Public => check_entity_scope(state, ctx, ScopeSource::Public, required).await,
    }
}

#[cfg(test)]
mod tests {
    use super::{ScopeSource, check_entity_scope, scope_matches};
    use crate::services::context::{AppContext, RequestSource};

    #[test]
    fn exact_and_wildcards() {
        assert!(scope_matches("kv.get", "kv.get"));
        assert!(scope_matches("kv.all", "kv.get"));
        assert!(scope_matches("items.all", "items.read"));
        assert!(scope_matches("rootaccess.all", "anything.at.all"));
        assert!(!scope_matches("kv.get", "kv.put"));
        assert!(!scope_matches("kv.all", "items.read"));
    }

    /// A plugin is bounded by its install's `granted_scopes` — the public role
    /// must not widen or narrow what the install was granted.
    #[tokio::test]
    async fn plugin_scopes_come_from_the_install_not_the_public_role() {
        let state = crate::utils::test_utils::get_app_state().await;
        let pool = state.database_pool.as_ref();
        let ctx = AppContext::system(RequestSource::API);

        let app_version_id: i32 = sqlx::query_scalar(
            "SELECT id FROM alcedocore.alcedocore_apps_versions ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .expect("test DB has at least one app version");

        // Clear any row a previously interrupted run left behind, otherwise the
        // unique slug constraint fails before the test even starts.
        sqlx::query("DELETE FROM alcedocore.alcedocore_plugins WHERE slug = 'scopes-fixture'")
            .execute(pool)
            .await
            .unwrap();

        let plugin_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins (slug, plugin_type, registry_id) \
             VALUES ('scopes-fixture', 'dynamic', 0) RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();

        let install_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins_installs \
             (plugin_id, app_version_id, plugin_version, enabled, granted_scopes) \
             VALUES ($1, $2, '1.0.0', true, '[\"kv.get\"]') RETURNING id",
        )
        .bind(plugin_id)
        .bind(app_version_id)
        .fetch_one(pool)
        .await
        .unwrap();
        let install_id = i64::from(install_id);

        // Granted.
        assert!(
            check_entity_scope(&state, &ctx, ScopeSource::Plugin(install_id), "kv.get")
                .await
                .is_ok(),
            "kv.get was granted to the install"
        );
        // Not granted — must not be inherited from the public role.
        assert!(
            check_entity_scope(&state, &ctx, ScopeSource::Plugin(install_id), "kv.put")
                .await
                .is_err(),
            "kv.put must not leak in from the public role"
        );
        // An install that no longer exists grants nothing.
        assert!(
            check_entity_scope(&state, &ctx, ScopeSource::Plugin(i64::MAX), "kv.get")
                .await
                .is_err(),
            "a missing install must fail closed"
        );

        sqlx::query("DELETE FROM alcedocore.alcedocore_plugins WHERE id = $1")
            .bind(plugin_id)
            .execute(pool)
            .await
            .unwrap();
    }
}
