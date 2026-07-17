use uuid::Uuid;

use crate::db::Pool;
use crate::error::AppError;
use sqlx::Row;

pub enum ScopeSource<'a> {
    Plugin { slug: &'a str },
    User { user_id: &'a Uuid },
    Public,
}

impl std::fmt::Debug for ScopeSource<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopeSource::Plugin { slug } => write!(f, "Plugin({})", slug),
            ScopeSource::User { user_id } => write!(f, "User({})", user_id),
            ScopeSource::Public => write!(f, "Public"),
        }
    }
}

pub async fn check_entity_scope(
    pool: &Pool,
    source: ScopeSource<'_>,
    required: &str,
) -> Result<(), AppError> {
    let granted = match source {
        ScopeSource::Plugin { slug } => {
            let row = sqlx::query(
                "SELECT granted_scopes FROM plugins WHERE slug = $1",
            )
            .bind(slug)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Plugin not found: {}", slug)))?;

            let scopes: serde_json::Value = row.get("granted_scopes");
            serde_json::from_value::<Vec<String>>(scopes).unwrap_or_default()
        }
        ScopeSource::User { user_id } => {
            sqlx::query_scalar::<_, String>(
                r#"SELECT DISTINCT rs.scope
                   FROM user_roles ur
                   JOIN role_scopes rs ON rs.role_id = ur.role_id
                   WHERE ur.user_id = $1"#,
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?
        }
        ScopeSource::Public => {
            sqlx::query_scalar::<_, String>(
                r#"SELECT rs.scope
                   FROM roles r
                   JOIN role_scopes rs ON rs.role_id = r.id
                   WHERE r.name = 'public'"#,
            )
            .fetch_all(pool)
            .await?
        }
    };

    if granted.iter().any(|g| scope_matches(g, required)) {
        Ok(())
    } else {
        Err(AppError::Forbidden(format!(
            "Missing required scope: {}",
            required
        )))
    }
}

/// Check whether a granted scope covers a required scope.
/// Supports wildcard: kv.all matches kv.get, kv.put, etc.
pub fn scope_matches(granted: &str, required: &str) -> bool {
    if granted == required {
        return true;
    }
    // rootaccess.all matches everything — super-admin wildcard
    if granted == "rootaccess.all" {
        return true;
    }
    // kv.all matches kv.get, kv.put, kv.exists, etc.
    if let Some(prefix) = granted.strip_suffix(".all") {
        if required.starts_with(&format!("{}.", prefix)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_matches_exact() {
        assert!(scope_matches("kv.get", "kv.get"));
    }

    #[test]
    fn test_scope_matches_wildcard() {
        assert!(scope_matches("kv.all", "kv.get"));
        assert!(scope_matches("kv.all", "kv.put"));
        assert!(scope_matches("items.all", "items.read"));
    }

    #[test]
    fn test_scope_matches_admin_wildcard() {
        assert!(scope_matches("users.all", "users.read"));
        assert!(scope_matches("users.all", "users.write"));
        assert!(scope_matches("plugins.all", "plugins.deploy"));
        assert!(scope_matches("settings.all", "settings.read.own"));
        assert!(scope_matches("settings.all", "settings.read.all"));
    }

    #[test]
    fn test_scope_matches_no_cross_prefix() {
        assert!(!scope_matches("kv.all", "items.read"));
        assert!(!scope_matches("items.all", "kv.get"));
    }

    #[test]
    fn test_scope_matches_no_false_positive() {
        assert!(!scope_matches("kv.get", "kv.put"));
    }
}
