use crate::{
    delete_by, error::AppError, find_all, find_all_where, find_all_where_bind, find_by,
    find_by_two, find_by_where, update_by_slug_version,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Plugin {
    pub slug: String,
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
    pub slug: String,
    pub version: String,
    pub container_id: Option<String>,
    pub status: String,
    pub is_active: bool,
    pub deployed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub public_synced: bool,
    pub public_path: Option<String>,
    pub pages_synced: bool,
    pub pages_path: Option<String>,
}

impl PluginVersion {
    find_by_where!(find_active, "plugin_versions", "slug, version, container_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path", "slug", "is_active = TRUE");

    pub async fn set_active(db: &PgPool, slug: &str, version: &str) -> Result<(), AppError> {
        let mut tx = db.begin().await?;

        sqlx::query(
            "UPDATE plugin_versions SET is_active = FALSE WHERE slug = $1 AND is_active = TRUE",
        )
        .bind(slug)
        .execute(&mut *tx)
        .await?;

        sqlx::query("UPDATE plugin_versions SET is_active = TRUE WHERE slug = $1 AND version = $2")
            .bind(slug)
            .bind(version)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn deactivate(db: &PgPool, slug: &str, version: &str) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE plugin_versions SET is_active = FALSE WHERE slug = $1 AND version = $2",
        )
        .bind(slug)
        .bind(version)
        .execute(db)
        .await?;
        Ok(())
    }

    pub async fn clear_container(db: &PgPool, slug: &str, version: &str) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE plugin_versions SET container_id = NULL, status = 'stopped' WHERE slug = $1 AND version = $2"
        )
        .bind(slug)
        .bind(version)
        .execute(db)
        .await?;
        Ok(())
    }

    pub async fn update_container(
        db: &PgPool,
        slug: &str,
        version: &str,
        container_id: &str,
        status: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE plugin_versions SET container_id = $3, status = $4 WHERE slug = $1 AND version = $2"
        )
        .bind(slug)
        .bind(version)
        .bind(container_id)
        .bind(status)
        .execute(db)
        .await?;
        Ok(())
    }

    update_by_slug_version!(update_status, "status");
    find_all_where!(find_draining, "plugin_versions", "slug, version, container_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path", "status = 'draining'", "deployed_at DESC");
    find_all!(find_all, "plugin_versions", "slug, version, container_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path", "deployed_at DESC");
    find_all_where!(find_all_active, "plugin_versions", "slug, version, container_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path", "is_active = TRUE", "deployed_at DESC");
    find_by_two!(find_by_slug_and_version, "plugin_versions", "slug, version, container_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path", "slug", "version");

    pub async fn set_public_synced(
        db: &PgPool,
        slug: &str,
        version: &str,
        synced: bool,
        path: Option<&str>,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE plugin_versions SET public_synced = $3, public_path = $4 WHERE slug = $1 AND version = $2"
        )
        .bind(slug)
        .bind(version)
        .bind(synced)
        .bind(path)
        .execute(db)
        .await?;
        Ok(())
    }

    pub async fn set_pages_synced(
        db: &PgPool,
        slug: &str,
        version: &str,
        synced: bool,
        path: Option<&str>,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE plugin_versions SET pages_synced = $3, pages_path = $4 WHERE slug = $1 AND version = $2"
        )
        .bind(slug)
        .bind(version)
        .bind(synced)
        .bind(path)
        .execute(db)
        .await?;
        Ok(())
    }

    pub async fn insert(db: &PgPool, version: &PluginVersion) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO plugin_versions (slug, version, container_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
        )
        .bind(&version.slug)
        .bind(&version.version)
        .bind(&version.container_id)
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

    find_all_where_bind!(find_all_by_slug, "plugin_versions", "slug, version, container_id, status, is_active, deployed_at, public_synced, public_path, pages_synced, pages_path", "slug = $1", "deployed_at DESC");
    update_by_slug_version!(update_container_id, "container_id");

    delete_by!(delete_all_for_slug, "plugin_versions", "slug", &str);
}

impl Plugin {
    find_by!(find_by_slug, "plugins", "slug, image, plugin_type, system_plugin, env, resources, display_name, description, pages, endpoints, documentation, settings_schema, settings, tags, enabled, requested_scopes, granted_scopes, registry_id, created_at, updated_at", "slug");

    pub async fn check_not_exists(db: &PgPool, slug: &str) -> Result<(), AppError> {
        let existing = Self::find_by_slug(db, slug).await?;
        if existing.is_some() {
            return Err(AppError::Conflict(format!(
                "Plugin {} already exists",
                slug
            )));
        }
        Ok(())
    }

    find_all!(find_all, "plugins", "slug, image, plugin_type, system_plugin, env, resources, display_name, description, pages, endpoints, documentation, settings_schema, settings, tags, enabled, requested_scopes, granted_scopes, registry_id, created_at, updated_at", "slug");

    find_all_where!(find_system_plugins, "plugins", "slug, image, plugin_type, system_plugin, env, resources, display_name, description, pages, endpoints, documentation, settings_schema, settings, tags, enabled, requested_scopes, granted_scopes, registry_id, created_at, updated_at", "system_plugin = TRUE", "slug");

    const PLUGIN_INSERT_SQL_BASE: &str = "INSERT INTO plugins (slug, image, plugin_type, system_plugin, env, resources, display_name, description, pages, endpoints, documentation, settings_schema, settings, tags, enabled, requested_scopes, granted_scopes, registry_id, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)";
    const PLUGIN_UPSERT_SUFFIX: &str = " ON CONFLICT (slug) DO UPDATE SET
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
        Self::plugin_insert_binds(db, plugin, Self::PLUGIN_UPSERT_SUFFIX).await
    }

    pub async fn update(
        db: &PgPool,
        slug: &str,
        display_name: Option<&String>,
        description: Option<&String>,
        tags: Option<&serde_json::Value>,
    ) -> Result<(), AppError> {
        let mut updates = Vec::new();
        let mut param_idx = 1;
        let mut has_updates = false;

        if let Some(_v) = display_name {
            updates.push(format!("display_name = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if let Some(_v) = description {
            updates.push(format!("description = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if let Some(_v) = tags {
            updates.push(format!("tags = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }

        if !has_updates {
            return Ok(());
        }

        updates.push(format!("updated_at = NOW()"));

        let query = format!(
            "UPDATE plugins SET {} WHERE slug = ${}",
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
        q = q.bind(slug);
        q.execute(db).await?;
        Ok(())
    }

    delete_by!(delete_by_slug, "plugins", "slug", &str);
}
