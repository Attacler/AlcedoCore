use crate::db::Pool;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

const PLUGIN_COLS: &str = "id, slug, app_version_id, version_id, image, plugin_type, system_plugin, env, resources, display_name, description, pages, endpoints, documentation, settings_schema, settings, tags, enabled, requested_scopes, granted_scopes, registry_id, created_at, updated_at";

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Plugin {
    #[sqlx(default)]
    pub id: i64,
    pub slug: String,
    #[sqlx(default)]
    pub app_version_id: Option<i32>,
    #[sqlx(default)]
    pub version_id: Option<i32>,
    pub image: String,
    pub plugin_type: String,
    pub system_plugin: bool,
    pub env: serde_json::Value,
    pub resources: serde_json::Value,
    pub display_name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    #[sqlx(default)]
    pub pages: serde_json::Value,
    pub endpoints: serde_json::Value,
    pub documentation: serde_json::Value,
    pub settings_schema: serde_json::Value,
    pub settings: serde_json::Value,
    pub tags: serde_json::Value,
    #[serde(default)]
    #[sqlx(default)]
    pub enabled: bool,
    #[serde(default)]
    #[sqlx(default)]
    pub requested_scopes: serde_json::Value,
    #[serde(default)]
    #[sqlx(default)]
    pub granted_scopes: serde_json::Value,
    #[serde(default)]
    #[sqlx(default)]
    pub registry_id: i32,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct PluginVersion {
    #[sqlx(default)]
    pub id: i64,
    #[sqlx(default)]
    pub install_id: i64,
    pub slug: String,
    pub version: String,
    pub deployment_id: Option<String>,
    pub status: String,
    pub is_active: bool,
    pub deployed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub public_synced: bool,
    pub public_path: Option<String>,
    pub pages_synced: bool,
    pub pages_path: Option<String>,
}

impl PluginVersion {
    pub async fn find_active_for_install(db: &PgPool, install_id: i64) -> Result<Option<Self>, AppError> {
        let row = sqlx::query_as::<_, Self>(
            "SELECT id, install_id, slug, version, deployment_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path
             FROM alcedo_plugin_versions WHERE install_id = $1 AND is_active = TRUE"
        )
        .bind(install_id)
        .fetch_optional(db)
        .await?;
        Ok(row)
    }

    pub async fn set_active(db: &PgPool, install_id: i64, version: &str) -> Result<(), AppError> {
        let mut tx = db.begin().await?;
        sqlx::query("UPDATE alcedo_plugin_versions SET is_active = FALSE WHERE install_id = $1 AND is_active = TRUE")
            .bind(install_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE alcedo_plugin_versions SET is_active = TRUE WHERE install_id = $1 AND version = $2")
            .bind(install_id)
            .bind(version)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn deactivate(db: &PgPool, install_id: i64, version: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE alcedo_plugin_versions SET is_active = FALSE WHERE install_id = $1 AND version = $2")
            .bind(install_id)
            .bind(version)
            .execute(db)
            .await?;
        Ok(())
    }

    pub async fn clear_deployment(db: &PgPool, install_id: i64, version: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE alcedo_plugin_versions SET deployment_id = NULL, status = 'stopped' WHERE install_id = $1 AND version = $2")
            .bind(install_id)
            .bind(version)
            .execute(db)
            .await?;
        Ok(())
    }

    pub async fn update_deployment(db: &PgPool, install_id: i64, version: &str, deployment_id: &str, status: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE alcedo_plugin_versions SET deployment_id = $3, status = $4 WHERE install_id = $1 AND version = $2")
            .bind(install_id)
            .bind(version)
            .bind(deployment_id)
            .bind(status)
            .execute(db)
            .await?;
        Ok(())
    }

    pub async fn update_status(db: &PgPool, install_id: i64, version: &str, status: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE alcedo_plugin_versions SET status = $3 WHERE install_id = $1 AND version = $2")
            .bind(install_id)
            .bind(version)
            .bind(status)
            .execute(db)
            .await?;
        Ok(())
    }

    pub async fn set_public_synced(db: &PgPool, install_id: i64, version: &str, synced: bool, path: Option<&str>) -> Result<(), AppError> {
        sqlx::query("UPDATE alcedo_plugin_versions SET public_synced = $3, public_path = $4 WHERE install_id = $1 AND version = $2")
            .bind(install_id)
            .bind(version)
            .bind(synced)
            .bind(path)
            .execute(db)
            .await?;
        Ok(())
    }

    pub async fn set_pages_synced(db: &PgPool, install_id: i64, version: &str, synced: bool, path: Option<&str>) -> Result<(), AppError> {
        sqlx::query("UPDATE alcedo_plugin_versions SET pages_synced = $3, pages_path = $4 WHERE install_id = $1 AND version = $2")
            .bind(install_id)
            .bind(version)
            .bind(synced)
            .bind(path)
            .execute(db)
            .await?;
        Ok(())
    }

    pub async fn insert(db: &PgPool, version: &PluginVersion) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO alcedo_plugin_versions (install_id, slug, version, deployment_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
        )
        .bind(version.install_id)
        .bind(&version.slug)
        .bind(&version.version)
        .bind(&version.deployment_id)
        .bind(&version.status)
        .bind(version.is_active)
        .bind(version.deployed_at)
        .bind(version.public_synced)
        .bind(&version.public_path)
        .bind(version.pages_synced)
        .bind(&version.pages_path)
        .execute(db)
        .await?;
        Ok(())
    }

    pub async fn find_by_install_and_version(db: &PgPool, install_id: i64, version: &str) -> Result<Option<Self>, AppError> {
        Ok(sqlx::query_as::<_, Self>(
            "SELECT id, install_id, slug, version, deployment_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path
             FROM alcedo_plugin_versions WHERE install_id = $1 AND version = $2"
        )
        .bind(install_id)
        .bind(version)
        .fetch_optional(db)
        .await?)
    }

    pub async fn find_all_by_install(db: &PgPool, install_id: i64) -> Result<Vec<Self>, AppError> {
        Ok(sqlx::query_as::<_, Self>(
            "SELECT id, install_id, slug, version, deployment_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path
             FROM alcedo_plugin_versions WHERE install_id = $1 ORDER BY deployed_at DESC"
        )
        .bind(install_id)
        .fetch_all(db)
        .await?)
    }

    pub async fn delete_all_for_install(db: &PgPool, install_id: i64) -> Result<(), AppError> {
        sqlx::query("DELETE FROM alcedo_plugin_versions WHERE install_id = $1").bind(install_id).execute(db).await?;
        Ok(())
    }
}

impl Plugin {
    /// Which install scope this row is: `app`, `version`, or `global`.
    pub fn scope(&self) -> &'static str {
        if self.app_version_id.is_some() {
            "app"
        } else if self.version_id.is_some() {
            "version"
        } else {
            "global"
        }
    }

    /// Look up a specific install by slug + scope. `app_version_id = None` = global.
    pub async fn find_install(
        db: &PgPool,
        slug: &str,
        app_version_id: Option<i32>,
    ) -> Result<Option<Self>, AppError> {
        Self::find_install_scoped(db, slug, app_version_id, None).await
    }

    /// Look up a specific install by slug + both scope keys. Only one key may be set.
    pub async fn find_install_scoped(
        db: &PgPool,
        slug: &str,
        app_version_id: Option<i32>,
        version_id: Option<i32>,
    ) -> Result<Option<Self>, AppError> {
        let sql = format!(
            "SELECT {} FROM alcedo_plugins \
             WHERE slug = $1 \
               AND app_version_id IS NOT DISTINCT FROM $2 \
               AND version_id IS NOT DISTINCT FROM $3",
            PLUGIN_COLS
        );
        Ok(sqlx::query_as::<_, Self>(&sql)
            .bind(slug)
            .bind(app_version_id)
            .bind(version_id)
            .fetch_optional(db)
            .await?)
    }

    /// Resolve an install for a request context: exact (slug, app_version) first,
    /// then global (slug, NULL). Returns None → caller should 404.
    pub async fn resolve_install(
        db: &PgPool,
        slug: &str,
        app_version_id: Option<i32>,
    ) -> Result<Option<Self>, AppError> {
        Self::resolve_install_scoped(db, slug, app_version_id, None).await
    }

    /// Most-specific-wins: app → version → global.
    pub async fn resolve_install_scoped(
        db: &PgPool,
        slug: &str,
        app_version_id: Option<i32>,
        version_id: Option<i32>,
    ) -> Result<Option<Self>, AppError> {
        if let Some(av) = app_version_id {
            if let Some(p) = Self::find_install_scoped(db, slug, Some(av), None).await? {
                return Ok(Some(p));
            }
        }
        if let Some(v) = version_id {
            if let Some(p) = Self::find_install_scoped(db, slug, None, Some(v)).await? {
                return Ok(Some(p));
            }
        }
        Self::find_install_scoped(db, slug, None, None).await
    }

    /// Reject a deploy when an install already exists at the exact scope.
    pub async fn check_install_not_exists_scoped(
        db: &PgPool,
        slug: &str,
        app_version_id: Option<i32>,
        version_id: Option<i32>,
    ) -> Result<(), AppError> {
        if Self::find_install_scoped(db, slug, app_version_id, version_id)
            .await?
            .is_some()
        {
            let scope = if app_version_id.is_some() {
                format!("app version {}", app_version_id.unwrap())
            } else if version_id.is_some() {
                format!("version {}", version_id.unwrap())
            } else {
                "global".to_string()
            };
            return Err(AppError::Conflict(format!(
                "Plugin {} already installed ({})",
                slug, scope
            )));
        }
        Ok(())
    }

    /// All installs of a slug (global + every app version). Used by list/detail.
    pub async fn find_all_installs(db: &PgPool, slug: &str) -> Result<Vec<Self>, AppError> {
        let sql = format!("SELECT {} FROM alcedo_plugins WHERE slug = $1 ORDER BY app_version_id NULLS FIRST, version_id NULLS FIRST", PLUGIN_COLS);
        Ok(sqlx::query_as::<_, Self>(&sql).bind(slug).fetch_all(db).await?)
    }

    /// Map an `alcedo_apps_versions.id` to its schema name
    /// (`{api_name}010{version_name}`), if it still exists.
    pub async fn resolve_schema_name_for_app_version(
        pool: &Pool,
        app_version_id: i32,
    ) -> Result<Option<String>, AppError> {
        let row: Option<(String, String)> = sqlx::query_as(
            r#"SELECT a.api_name, v.version_name
               FROM alcedo.alcedo_apps_versions av
               JOIN alcedo.alcedo_apps a ON a.id = av.app_id
               JOIN alcedo.alcedo_versions v ON v.id = av.version_id
               WHERE av.id = $1"#,
        )
        .bind(app_version_id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|(api_name, version)| {
            let ctx = alcedo_common::context::AppContext {
                app_name: api_name,
                version,
                request_source: alcedo_common::context::RequestSource::API,
            };
            ctx.schema_name()
        }))
    }

    pub async fn find_by_id(db: &PgPool, id: i64) -> Result<Option<Self>, AppError> {
        let sql = format!("SELECT {} FROM alcedo_plugins WHERE id = $1", PLUGIN_COLS);
        Ok(sqlx::query_as::<_, Self>(&sql).bind(id).fetch_optional(db).await?)
    }

    /// Legacy scope check: only detects `app`- and `global`-scoped installs,
    /// because it ignores `version_id`. Use [`Self::check_install_not_exists_scoped`]
    /// to also reject an existing version-scoped install.
    pub async fn check_install_not_exists(
        db: &PgPool,
        slug: &str,
        app_version_id: Option<i32>,
    ) -> Result<(), AppError> {
        if Self::find_install(db, slug, app_version_id).await?.is_some() {
            let scope = match app_version_id {
                Some(av) => format!("app version {}", av),
                None => "global".to_string(),
            };
            return Err(AppError::Conflict(format!(
                "Plugin {} already installed ({})",
                slug, scope
            )));
        }
        Ok(())
    }

    pub async fn find_all(db: &PgPool) -> Result<Vec<Self>, AppError> {
        let sql = format!("SELECT {} FROM alcedo_plugins ORDER BY slug, app_version_id NULLS FIRST, version_id NULLS FIRST", PLUGIN_COLS);
        Ok(sqlx::query_as::<_, Self>(&sql).fetch_all(db).await?)
    }

    pub async fn find_system_plugins(db: &PgPool) -> Result<Vec<Self>, AppError> {
        let sql = format!("SELECT {} FROM alcedo_plugins WHERE system_plugin = TRUE ORDER BY slug", PLUGIN_COLS);
        Ok(sqlx::query_as::<_, Self>(&sql).fetch_all(db).await?)
    }

    const PLUGIN_INSERT_SQL_BASE: &str = "INSERT INTO alcedo_plugins (slug, app_version_id, version_id, image, plugin_type, system_plugin, env, resources, display_name, description, pages, endpoints, documentation, settings_schema, settings, tags, enabled, requested_scopes, granted_scopes, registry_id, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22)";

    const PLUGIN_UPSERT_SUFFIX_VERSIONED: &str = " ON CONFLICT (slug, app_version_id) WHERE app_version_id IS NOT NULL DO UPDATE SET
               image = EXCLUDED.image,
               plugin_type = EXCLUDED.plugin_type,
               system_plugin = EXCLUDED.system_plugin,
               env = EXCLUDED.env,
               resources = EXCLUDED.resources,
               display_name = EXCLUDED.display_name,
               description = EXCLUDED.description,
               pages = EXCLUDED.pages,
               endpoints = EXCLUDED.endpoints,
               documentation = EXCLUDED.documentation,
               settings_schema = EXCLUDED.settings_schema,
               settings = EXCLUDED.settings,
               tags = EXCLUDED.tags,
               enabled = EXCLUDED.enabled,
               requested_scopes = EXCLUDED.requested_scopes,
               granted_scopes = EXCLUDED.granted_scopes,
               registry_id = EXCLUDED.registry_id,
               updated_at = NOW()";

    const PLUGIN_UPSERT_SUFFIX_GLOBAL: &str = " ON CONFLICT (slug) WHERE app_version_id IS NULL AND version_id IS NULL DO UPDATE SET
               image = EXCLUDED.image,
               plugin_type = EXCLUDED.plugin_type,
               system_plugin = EXCLUDED.system_plugin,
               env = EXCLUDED.env,
               resources = EXCLUDED.resources,
               display_name = EXCLUDED.display_name,
               description = EXCLUDED.description,
               pages = EXCLUDED.pages,
               endpoints = EXCLUDED.endpoints,
               documentation = EXCLUDED.documentation,
               settings_schema = EXCLUDED.settings_schema,
               settings = EXCLUDED.settings,
               tags = EXCLUDED.tags,
               enabled = EXCLUDED.enabled,
               requested_scopes = EXCLUDED.requested_scopes,
               granted_scopes = EXCLUDED.granted_scopes,
               registry_id = EXCLUDED.registry_id,
               updated_at = NOW()";

    const PLUGIN_UPSERT_SUFFIX_VERSION_SCOPED: &str = " ON CONFLICT (slug, version_id) WHERE version_id IS NOT NULL DO UPDATE SET
               image = EXCLUDED.image,
               plugin_type = EXCLUDED.plugin_type,
               system_plugin = EXCLUDED.system_plugin,
               env = EXCLUDED.env,
               resources = EXCLUDED.resources,
               display_name = EXCLUDED.display_name,
               description = EXCLUDED.description,
               pages = EXCLUDED.pages,
               endpoints = EXCLUDED.endpoints,
               documentation = EXCLUDED.documentation,
               settings_schema = EXCLUDED.settings_schema,
               settings = EXCLUDED.settings,
               tags = EXCLUDED.tags,
               enabled = EXCLUDED.enabled,
               requested_scopes = EXCLUDED.requested_scopes,
               granted_scopes = EXCLUDED.granted_scopes,
               registry_id = EXCLUDED.registry_id,
               updated_at = NOW()";

    async fn plugin_insert_binds(
        db: &PgPool,
        plugin: &Plugin,
        upsert_suffix: &str,
    ) -> Result<(), AppError> {
        let sql = format!("{}{}", Self::PLUGIN_INSERT_SQL_BASE, upsert_suffix);
        sqlx::query(&sql)
            .bind(&plugin.slug)
            .bind(plugin.app_version_id)
            .bind(plugin.version_id)
            .bind(&plugin.image)
            .bind(&plugin.plugin_type)
            .bind(plugin.system_plugin)
            .bind(&plugin.env)
            .bind(&plugin.resources)
            .bind(&plugin.display_name)
            .bind(&plugin.description)
            .bind(&plugin.pages)
            .bind(&plugin.endpoints)
            .bind(&plugin.documentation)
            .bind(&plugin.settings_schema)
            .bind(&plugin.settings)
            .bind(&plugin.tags)
            .bind(plugin.enabled)
            .bind(&plugin.requested_scopes)
            .bind(&plugin.granted_scopes)
            .bind(plugin.registry_id)
            .bind(plugin.created_at)
            .bind(plugin.updated_at)
            .execute(db)
            .await?;
        Ok(())
    }

    pub async fn insert(db: &PgPool, plugin: &Plugin) -> Result<(), AppError> {
        Self::plugin_insert_binds(db, plugin, "").await
    }

    pub async fn upsert(db: &PgPool, plugin: &Plugin) -> Result<(), AppError> {
        let suffix = if plugin.app_version_id.is_some() {
            Self::PLUGIN_UPSERT_SUFFIX_VERSIONED
        } else if plugin.version_id.is_some() {
            Self::PLUGIN_UPSERT_SUFFIX_VERSION_SCOPED
        } else {
            Self::PLUGIN_UPSERT_SUFFIX_GLOBAL
        };
        Self::plugin_insert_binds(db, plugin, suffix).await
    }

    pub async fn update_by_id(
        db: &PgPool,
        id: i64,
        display_name: Option<&String>,
        description: Option<&String>,
        tags: Option<&serde_json::Value>,
    ) -> Result<(), AppError> {
        let mut updates = Vec::new();
        let mut param_idx = 1;
        let mut has_updates = false;
        if display_name.is_some() {
            updates.push(format!("display_name = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if description.is_some() {
            updates.push(format!("description = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if tags.is_some() {
            updates.push(format!("tags = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if !has_updates {
            return Ok(());
        }
        updates.push("updated_at = NOW()".to_string());
        let query = format!(
            "UPDATE alcedo_plugins SET {} WHERE id = ${}",
            updates.join(", "),
            param_idx
        );
        let mut q = sqlx::query(&query);
        if let Some(v) = display_name {
            q = q.bind(v);
        }
        if let Some(v) = description {
            q = q.bind(v);
        }
        if let Some(v) = tags {
            q = q.bind(v);
        }
        q = q.bind(id);
        q.execute(db).await?;
        Ok(())
    }

    pub async fn delete_by_id(db: &PgPool, id: i64) -> Result<(), AppError> {
        sqlx::query("DELETE FROM alcedo_plugins WHERE id = $1").bind(id).execute(db).await?;
        Ok(())
    }
}
