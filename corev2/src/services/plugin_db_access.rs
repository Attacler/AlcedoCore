use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;
use sqlx::{Pool, Postgres, Row};

use crate::services::{
    app_state::AppState,
    encryption,
    errors::AlcedoError,
    plugin_migrations::{plugin_role_name, plugin_schema_name},
};

pub const DB_ACCESS_TABLE: &str = "alcedocore.alcedocore_plugin_db_access";

#[derive(Debug, Clone)]
pub struct InstallDbAccess {
    pub install_id: i64,
    pub schema_name: String,
    pub role_name: String,
    pub password_enc: String,
}

pub async fn get_install_db_access(
    pool: &Pool<Postgres>,
    install_id: i64,
) -> Result<Option<InstallDbAccess>, AlcedoError> {
    let row = sqlx::query(&format!(
        "SELECT install_id, schema_name, role_name, password_enc \
         FROM {DB_ACCESS_TABLE} WHERE install_id = $1"
    ))
    .bind(install_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| InstallDbAccess {
        install_id: r.get("install_id"),
        schema_name: r.get("schema_name"),
        role_name: r.get("role_name"),
        password_enc: r.get("password_enc"),
    }))
}

/// Creates the install's schema + LOGIN role (idempotent) and records the
/// encrypted password. The role gets USAGE and ALL on its own schema only —
/// outside it Postgres itself answers `permission denied for schema ...`.
pub async fn ensure_install_db_access(
    pool: &Pool<Postgres>,
    install_id: i64,
) -> Result<InstallDbAccess, AlcedoError> {
    if let Some(existing) = get_install_db_access(pool, install_id).await? {
        return Ok(existing);
    }

    let schema = plugin_schema_name(install_id);
    let role = plugin_role_name(install_id);
    // Base64 alphabet has no quote characters, so the password is safe to
    // inline; role/schema names are derived from the numeric install id.
    let mut password_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut password_bytes);
    let password = BASE64.encode(password_bytes);
    let password_enc = encryption::encrypt_with(&password, encryption::get_plugin_access_key()?)?;

    let mut tx = pool.begin().await?;
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\""))
        .execute(&mut *tx)
        .await?;
    sqlx::query(&format!("REVOKE ALL ON SCHEMA \"{schema}\" FROM PUBLIC"))
        .execute(&mut *tx)
        .await?;
    sqlx::query(&format!(
        "DO $$ BEGIN \
         IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{role}') THEN \
         CREATE ROLE \"{role}\" LOGIN PASSWORD '{password}'; \
         END IF; END $$;"
    ))
    .execute(&mut *tx)
    .await?;
    // Re-set the password unconditionally: if the role outlived a previous
    // access row, the stored credential must match the role.
    sqlx::query(&format!(
        "ALTER ROLE \"{role}\" LOGIN PASSWORD '{password}'"
    ))
    .execute(&mut *tx)
    .await?;
    // ALL = USAGE + CREATE: the plugin creates its own tables at runtime.
    sqlx::query(&format!("GRANT ALL ON SCHEMA \"{schema}\" TO \"{role}\""))
        .execute(&mut *tx)
        .await?;
    sqlx::query(&format!(
        "GRANT ALL ON ALL TABLES IN SCHEMA \"{schema}\" TO \"{role}\""
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!(
        "GRANT ALL ON ALL SEQUENCES IN SCHEMA \"{schema}\" TO \"{role}\""
    ))
    .execute(&mut *tx)
    .await?;
    // Tables the plugin's own migrations create later.
    sqlx::query(&format!(
        "ALTER DEFAULT PRIVILEGES IN SCHEMA \"{schema}\" GRANT ALL ON TABLES TO \"{role}\""
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!(
        "ALTER DEFAULT PRIVILEGES IN SCHEMA \"{schema}\" GRANT ALL ON SEQUENCES TO \"{role}\""
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!(
        "ALTER ROLE \"{role}\" SET search_path = '{schema}'"
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!(
        "INSERT INTO {DB_ACCESS_TABLE} (install_id, schema_name, role_name, password_enc) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (install_id) DO UPDATE SET \
             schema_name = EXCLUDED.schema_name, \
             role_name = EXCLUDED.role_name, \
             password_enc = EXCLUDED.password_enc"
    ))
    .bind(install_id)
    .bind(&schema)
    .bind(&role)
    .bind(&password_enc)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(InstallDbAccess {
        install_id,
        schema_name: schema,
        role_name: role,
        password_enc,
    })
}

/// Drops the install's schema and LOGIN role. Names are derived from the
/// install id, so this works after the access row has already cascaded away.
pub async fn drop_install_db_access(
    pool: &Pool<Postgres>,
    install_id: i64,
) -> Result<(), AlcedoError> {
    let schema = plugin_schema_name(install_id);
    let role = plugin_role_name(install_id);
    let mut tx = pool.begin().await?;
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(&mut *tx)
        .await?;
    sqlx::query(&format!("DROP OWNED BY \"{role}\""))
        .execute(&mut *tx)
        .await?;
    sqlx::query(&format!("DROP ROLE IF EXISTS \"{role}\""))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// The core's `DATABASE_URL` with the install role's credentials swapped in.
pub fn build_scoped_url(core_url: &str, role: &str, password: &str) -> Result<String, AlcedoError> {
    let mut url = url::Url::parse(core_url)
        .map_err(|e| AlcedoError::SystemError(format!("Invalid DATABASE_URL: {e}"), 0))?;
    url.set_username(role).map_err(|_| {
        AlcedoError::SystemError("DATABASE_URL cannot carry a username".to_string(), 0)
    })?;
    url.set_password(Some(password)).map_err(|_| {
        AlcedoError::SystemError("DATABASE_URL cannot carry a password".to_string(), 0)
    })?;
    Ok(url.to_string())
}

/// The scoped connection string for an install, or `None` for legacy installs
/// that predate per-install roles.
pub async fn scoped_database_url(
    state: &AppState,
    install_id: i64,
) -> Result<Option<String>, AlcedoError> {
    let Some(access) = get_install_db_access(&state.database_pool, install_id).await? else {
        return Ok(None);
    };
    let password =
        encryption::decrypt_with(&access.password_enc, encryption::get_plugin_access_key()?)?;
    Ok(Some(build_scoped_url(
        &state.config.database_url,
        &access.role_name,
        &password,
    )?))
}

/// Applies the scoped `DATABASE_URL` to a plugin's env **after** the deploy
/// payload's own `env` was merged, so a payload can never override it with the
/// core's full-privilege URL. Installs without an access row (legacy) get any
/// payload-provided `DATABASE_URL` stripped instead.
pub async fn apply_scoped_database_url(
    state: &AppState,
    env: &mut HashMap<String, String>,
    install_id: i64,
) {
    match scoped_database_url(state, install_id).await {
        Ok(Some(url)) => {
            env.insert("DATABASE_URL".to_string(), url);
        }
        Ok(None) => {
            env.remove("DATABASE_URL");
        }
        Err(e) => {
            tracing::warn!(
                "[PLUGINS] Could not build scoped DATABASE_URL for install {}: {}",
                install_id,
                e
            );
            env.remove("DATABASE_URL");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item_map;
    use crate::services::{
        context::{AppContext, RequestSource},
        items::service::ItemsService,
        plugin_migrations::PluginMigrationEngine,
    };
    use serde_json::Value;

    // Must match the value the encryption tests set, so the process-global
    // OnceLock agrees no matter which test runs first.
    const TEST_PLUGIN_KEY: &str = "BuCUYpRBM9tByB6vwG/1uOYBCc4GzZ0OBL1BoOtkaWI=";

    fn set_plugin_key() {
        unsafe {
            std::env::set_var("PLUGIN_ACCESS_ENCRYPTION_KEY", TEST_PLUGIN_KEY);
        }
    }

    #[test]
    fn scoped_url_swaps_only_the_credentials() {
        let url = build_scoped_url(
            "postgres://postgres:postgres@0.0.0.0:5432/plugin_core",
            "alcedo_p7",
            "pw123",
        )
        .unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.username(), "alcedo_p7");
        assert_eq!(parsed.password(), Some("pw123"));
        assert_eq!(parsed.host_str(), Some("0.0.0.0"));
        assert_eq!(parsed.port(), Some(5432));
        assert_eq!(parsed.path(), "/plugin_core");
        // Special characters are percent-encoded (the url crate's job); sqlx
        // decodes them when connecting, which the integration test proves.
        let url = build_scoped_url(
            "postgres://postgres:postgres@0.0.0.0:5432/plugin_core",
            "alcedo_p7",
            "pw/123+=",
        )
        .unwrap();
        assert!(url.contains("pw%2F123+%3D"));
    }

    #[test]
    fn role_and_schema_names_are_install_scoped() {
        assert_eq!(plugin_role_name(42), "alcedo_p42");
        assert_eq!(plugin_schema_name(42), "plugin_install_42");
    }

    /// Creates a global install row (registry + catalog + install) so the
    /// access row's FK is satisfied, and returns the install id.
    async fn create_install(state: &AppState) -> (i64, i64, i64) {
        let ctx = AppContext::system(RequestSource::SystemTest);
        let registry_id: i64 = {
            let collection = "alcedocore_registries".to_string();
            let mut svc = ItemsService::new(state, &ctx, &collection);
            let id = uuid::Uuid::new_v4().to_string();
            svc.create_many(
                vec![item_map! {
                    "id" => id.clone(),
                    "name" => format!("dbaccess-{id}"),
                    "url" => "http://localhost:5000",
                    "auth_type" => "none",
                }],
                &mut None,
            )
            .await
            .unwrap()
            .first()
            .and_then(|pk| pk.parse::<i64>().ok())
            .unwrap()
        };
        let plugin_id: i64 = {
            let collection = "alcedocore_plugins".to_string();
            let mut svc = ItemsService::new(state, &ctx, &collection);
            let slug = format!("dbaccess-{}", uuid::Uuid::new_v4());
            svc.create_many(
                vec![item_map! {
                    "slug" => slug,
                    "plugin_type" => "dynamic",
                    "registry_id" => registry_id,
                }],
                &mut None,
            )
            .await
            .unwrap()
            .first()
            .and_then(|pk| pk.parse::<i64>().ok())
            .unwrap()
        };
        let install_id: i64 = {
            let collection = "alcedocore_plugins_installs".to_string();
            let mut svc = ItemsService::new(state, &ctx, &collection);
            svc.create_many(
                vec![item_map! {
                    "plugin_id" => plugin_id,
                    "app_version_id" => Value::Null,
                    "plugin_version" => "0.0.0",
                    "enabled" => false,
                }],
                &mut None,
            )
            .await
            .unwrap()
            .first()
            .and_then(|pk| pk.parse::<i64>().ok())
            .unwrap()
        };
        (registry_id, plugin_id, install_id)
    }

    async fn cleanup_install(state: &AppState, registry_id: i64, plugin_id: i64, install_id: i64) {
        let pool = &*state.database_pool;
        let _ = drop_install_db_access(pool, install_id).await;
        sqlx::query("DELETE FROM alcedocore.alcedocore_plugins_installs WHERE id = $1")
            .bind(install_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM alcedocore.alcedocore_plugins WHERE id = $1")
            .bind(plugin_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM alcedocore.alcedocore_registries WHERE id = $1")
            .bind(registry_id)
            .execute(pool)
            .await
            .unwrap();
    }

    /// End-to-end: ensure creates schema+role+row; connecting as the role can
    /// write its own schema but gets `permission denied` everywhere else;
    /// drop removes schema+role.
    #[tokio::test]
    async fn install_role_is_scoped_to_its_own_schema() {
        set_plugin_key();
        let state = crate::utils::test_utils::get_app_state().await;
        let pool = &*state.database_pool;

        let (registry_id, plugin_id, install_id) = create_install(&state).await;
        let schema = plugin_schema_name(install_id);
        let role = plugin_role_name(install_id);

        let access = ensure_install_db_access(pool, install_id).await.unwrap();
        assert_eq!(access.schema_name, schema);
        assert_eq!(access.role_name, role);
        // Idempotent: a second ensure returns the same row.
        let again = ensure_install_db_access(pool, install_id).await.unwrap();
        assert_eq!(again.password_enc, access.password_enc);
        // The stored password decrypts under the plugin-access key.
        let password = encryption::decrypt_with(
            &access.password_enc,
            encryption::get_plugin_access_key().unwrap(),
        )
        .unwrap();
        assert!(!password.is_empty());

        // Connect as the role through the scoped URL.
        let scoped = scoped_database_url(&state, install_id)
            .await
            .unwrap()
            .unwrap();
        let scoped_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&scoped)
            .await
            .unwrap();

        // search_path is pinned to the install schema: unqualified DDL lands there.
        sqlx::query("CREATE TABLE probe (id serial PRIMARY KEY, v text)")
            .execute(&scoped_pool)
            .await
            .unwrap();
        let in_schema: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'probe')",
        )
        .bind(&schema)
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(in_schema, "role must create tables in its own schema");
        sqlx::query("INSERT INTO probe (v) VALUES ('x')")
            .execute(&scoped_pool)
            .await
            .unwrap();

        // The global schema and app schemas are unreachable.
        let err = sqlx::query("SELECT * FROM alcedocore.alcedocore_users LIMIT 1")
            .fetch_one(&scoped_pool)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("permission denied"),
            "global schema must be denied, got: {err}"
        );
        let err = sqlx::query("SELECT * FROM crm010production.contacts LIMIT 1")
            .fetch_one(&scoped_pool)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("permission denied"),
            "app schema must be denied, got: {err}"
        );

        // Close the role's sessions before dropping it.
        scoped_pool.close().await;
        drop_install_db_access(pool, install_id).await.unwrap();

        let schema_gone: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
        )
        .bind(&schema)
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(schema_gone);
        let role_gone: bool =
            sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = $1)")
                .bind(&role)
                .fetch_one(pool)
                .await
                .unwrap();
        assert!(role_gone);

        cleanup_install(&state, registry_id, plugin_id, install_id).await;
        // The access row cascaded with the install row.
        assert!(
            get_install_db_access(pool, install_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn migrations_run_in_install_schema_and_role_can_use_them() {
        set_plugin_key();
        let state = crate::utils::test_utils::get_app_state().await;
        let pool = &*state.database_pool;

        let (registry_id, plugin_id, install_id) = create_install(&state).await;
        ensure_install_db_access(pool, install_id).await.unwrap();
        let schema = plugin_schema_name(install_id);

        let dir = std::env::temp_dir().join(format!("alcedo-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("001_init.up.sql"),
            "CREATE TABLE mig_probe (id serial PRIMARY KEY, v text); \
             INSERT INTO mig_probe (v) VALUES ('a'), ('b');",
        )
        .unwrap();
        std::fs::write(dir.join("001_init.down.sql"), "DROP TABLE mig_probe;").unwrap();

        let engine = PluginMigrationEngine::new(&state, install_id, dir.clone());
        let result = engine.run().await.unwrap();
        assert!(result.errors.is_empty());
        assert_eq!(result.applied, vec!["001".to_string()]);
        // ensure_schema is idempotent: a second run applies nothing, no error.
        let again = engine.run().await.unwrap();
        assert!(again.applied.is_empty() && again.errors.is_empty());

        // The table landed in the install schema, not public.
        let in_schema: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'mig_probe')",
        )
        .bind(&schema)
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(in_schema);

        // The role can read/write the migration-created table via the scoped URL.
        let scoped = scoped_database_url(&state, install_id)
            .await
            .unwrap()
            .unwrap();
        let scoped_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&scoped)
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mig_probe")
            .fetch_one(&scoped_pool)
            .await
            .unwrap();
        assert_eq!(n, 2);
        sqlx::query("INSERT INTO mig_probe (v) VALUES ('c')")
            .execute(&scoped_pool)
            .await
            .unwrap();
        let denied = sqlx::query("SELECT * FROM alcedocore.alcedocore_users LIMIT 1")
            .fetch_one(&scoped_pool)
            .await
            .unwrap_err()
            .to_string();
        assert!(denied.contains("permission denied"), "got: {denied}");
        scoped_pool.close().await;

        // Rollback drops the table and removes the schema_migrations row.
        let rolled = engine.rollback_to("0").await.unwrap();
        assert_eq!(rolled, vec!["001".to_string()]);
        let gone: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_name = 'mig_probe')",
        )
        .bind(&schema)
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(gone);
        let remaining: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM \"{schema}\".schema_migrations"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0);

        std::fs::remove_dir_all(&dir).ok();
        cleanup_install(&state, registry_id, plugin_id, install_id).await;
    }

    /// The env injection overrides a payload-provided DATABASE_URL when an
    /// access row exists, and strips it when it does not.
    #[tokio::test]
    async fn apply_scoped_database_url_overrides_then_strips() {
        set_plugin_key();
        let state = crate::utils::test_utils::get_app_state().await;
        let pool = &*state.database_pool;

        let (registry_id, plugin_id, install_id) = create_install(&state).await;
        ensure_install_db_access(pool, install_id).await.unwrap();

        let mut env = HashMap::from([(
            "DATABASE_URL".to_string(),
            "postgres://postgres:postgres@db:5432/plugin_core".to_string(),
        )]);
        apply_scoped_database_url(&state, &mut env, install_id).await;
        let url = url::Url::parse(env.get("DATABASE_URL").unwrap()).unwrap();
        assert_eq!(url.username(), plugin_role_name(install_id));
        assert_ne!(url.password(), Some("postgres"));

        // No access row (legacy install): the payload URL is stripped, never passed through.
        let legacy_id = install_id + 1_000_000;
        let mut env = HashMap::from([(
            "DATABASE_URL".to_string(),
            "postgres://postgres:postgres@db:5432/plugin_core".to_string(),
        )]);
        apply_scoped_database_url(&state, &mut env, legacy_id).await;
        assert!(!env.contains_key("DATABASE_URL"));

        cleanup_install(&state, registry_id, plugin_id, install_id).await;
    }
}
