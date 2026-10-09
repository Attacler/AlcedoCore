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
        // Plugins fall back to the public role until `granted_scopes` are
        // enforced per install — see the plugin scope enforcement follow-up.
        // No plugin gets more than an anonymous caller already does.
        AuthLevel::Public | AuthLevel::Plugin(_) => {
            check_entity_scope(state, ctx, ScopeSource::Public, required).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scope_matches;

    #[test]
    fn exact_and_wildcards() {
        assert!(scope_matches("kv.get", "kv.get"));
        assert!(scope_matches("kv.all", "kv.get"));
        assert!(scope_matches("items.all", "items.read"));
        assert!(scope_matches("rootaccess.all", "anything.at.all"));
        assert!(!scope_matches("kv.get", "kv.put"));
        assert!(!scope_matches("kv.all", "items.read"));
    }
}
