use crate::{delete_by, error::AppError, find_all};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Registry {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub pull_url: Option<String>,
    pub auth_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Registry {
    pub async fn find_by_id(db: &PgPool, id: i32) -> Result<Option<Self>, AppError> {
        let row = sqlx::query_as::<_, Registry>(
            "SELECT id, name, url, pull_url, auth_type, username, password, created_at, updated_at
             FROM registries WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(db)
        .await?;
        Ok(row)
    }

    find_all!(
        find_all,
        "registries",
        "id, name, url, pull_url, auth_type, username, password, created_at, updated_at",
        "name"
    );

    pub async fn insert(db: &PgPool, registry: &Registry) -> Result<i32, AppError> {
        let password = if let Some(ref pw) = registry.password {
            Some(crate::services::encryption::encrypt(pw)?)
        } else {
            None
        };
        let row: (i32,) = sqlx::query_as(
            "INSERT INTO registries (name, url, pull_url, auth_type, username, password, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id"
        )
        .bind(&registry.name)
        .bind(&registry.url)
        .bind(&registry.pull_url)
        .bind(&registry.auth_type)
        .bind(&registry.username)
        .bind(&password)
        .bind(registry.created_at)
        .bind(registry.updated_at)
        .fetch_one(db)
        .await?;
        Ok(row.0)
    }

    pub async fn update(
        db: &PgPool,
        id: i32,
        name: Option<&String>,
        url: Option<&String>,
        pull_url: Option<&String>,
        auth_type: Option<&String>,
        username: Option<&String>,
        password: Option<&String>,
    ) -> Result<(), AppError> {
        let mut updates = Vec::new();
        let mut param_idx = 1;
        let mut has_updates = false;

        if let Some(_v) = name {
            updates.push(format!("name = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if let Some(_v) = url {
            updates.push(format!("url = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if let Some(_v) = auth_type {
            updates.push(format!("auth_type = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if let Some(_v) = username {
            updates.push(format!("username = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }
        if pull_url.is_some() {
            updates.push(format!("pull_url = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
        }

        let encrypted_password = if let Some(v) = password {
            updates.push(format!("password = ${}", param_idx));
            param_idx += 1;
            has_updates = true;
            Some(crate::services::encryption::encrypt(v)?)
        } else {
            None
        };

        if !has_updates {
            return Ok(());
        }

        updates.push(format!("updated_at = NOW()"));

        let query = format!(
            "UPDATE registries SET {} WHERE id = ${}",
            updates.join(", "),
            param_idx
        );

        let mut q = sqlx::query(&query);
        if let Some(v) = name {
            q = q.bind(v);
        }
        if let Some(v) = url {
            q = q.bind(v);
        }
        if let Some(v) = auth_type {
            q = q.bind(v);
        }
        if let Some(v) = username {
            q = q.bind(v);
        }
        if let Some(v) = pull_url {
            q = q.bind(v);
        }
        if let Some(ref v) = encrypted_password {
            q = q.bind(v);
        }
        q = q.bind(id);
        q.execute(db).await?;
        Ok(())
    }

    delete_by!(delete_by_id, "registries", "id", i32);

    /// Resolve a plugin image reference to a fully-qualified pull name for this
    /// registry. Pulls ALWAYS go through a configured registry:
    ///
    /// - Any leading registry-host segment in the supplied image (the first
    ///   path segment containing a '.' or ':') is replaced with the registry's
    ///   pull host (`pull_url` when set, otherwise `url`, scheme + trailing
    ///   slash stripped). E.g. `localhost:5000/hello-world:1.0.0` becomes
    ///   `<pull_host>/hello-world:1.0.0`.
    /// - Bare references (e.g. `hello-world:1.0.0` or `repo/nginx`) are
    ///   prefixed with the pull host.
    ///
    /// The input is returned unchanged only when the registry has no usable
    /// URL (defensive; a configured registry is expected to always exist).
    pub fn resolve_image(&self, image: &str) -> String {
        let pull_url_str = self.pull_url.clone().unwrap_or(self.url.clone());
        let pull_host = pull_url_str
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');

        if pull_host.is_empty() {
            return image.to_string();
        }

        // Replace any leading registry-host segment, if present.
        let first_slash = image.find('/');
        let first_segment: &str = match first_slash {
            Some(pos) => &image[..pos],
            None => "",
        };
        let rest: &str = if !first_segment.is_empty()
            && (first_segment.contains('.') || first_segment.contains(':'))
        {
            let pos = first_slash.unwrap();
            &image[pos + 1..]
        } else {
            image
        };

        format!("{}/{}", pull_host, rest)
    }

    /// True when this registry is the auto-created default placeholder
    /// (inserted by `ensure_default` / core migration 047) and so may be
    /// replaced by an env/startup-seeded registry.
    fn is_default_placeholder(&self) -> bool {
        self.name == "local"
            && self.url == "http://localhost:5000"
            && self.auth_type == "none"
            && self.pull_url.is_none()
            && self.username.is_none()
    }

    /// Ensure at least one registry exists so plugin pulls never have an
    /// "optional" registry code path.
    ///
    /// - Empty table → insert a `local` registry from `local_registry_url`
    ///   (e.g. `LOCAL_REGISTRY_URL`, default `localhost:5000`).
    /// - Table holds only the auto-created placeholder → upgrade its URL to
    ///   `local_registry_url` so Docker/Swarm and K8s find the right host.
    /// - Otherwise → leave existing registries untouched.
    ///
    /// Returns the id of the default (lowest-id) registry.
    pub async fn ensure_default(
        db: &PgPool,
        local_registry_url: &str,
    ) -> Result<i32, AppError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM registries")
            .fetch_one(db)
            .await?;

        let url = if local_registry_url.contains("://") {
            local_registry_url.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", local_registry_url.trim_end_matches('/'))
        };

        if count == 0 {
            let now = chrono::Utc::now();
            let id = Registry::insert(
                db,
                &Registry {
                    id: 0,
                    name: "local".to_string(),
                    url,
                    pull_url: None,
                    auth_type: "none".to_string(),
                    username: None,
                    password: None,
                    created_at: Some(now),
                    updated_at: Some(now),
                },
            )
            .await?;
            return Ok(id);
        }

        // Upgrade the sole auto-created placeholder to the configured host.
        if count == 1 {
            let all = Self::find_all(db).await?;
            if let Some(reg) = all.first() {
                if reg.is_default_placeholder() && reg.url != url {
                    Registry::update(
                        db,
                        reg.id,
                        None,
                        Some(&url),
                        None,
                        None,
                        None,
                        None,
                    )
                    .await?;
                    return Ok(reg.id);
                }
            }
        }

        // Return the default (lowest-id) registry.
        let row: (i32,) = sqlx::query_as("SELECT id FROM registries ORDER BY id LIMIT 1")
            .fetch_one(db)
            .await?;
        Ok(row.0)
    }

    /// Insert a registry from env config on startup, but only when the
    /// registries table is currently empty (idempotent across restarts).
    /// A lone auto-created placeholder is upgraded in place instead (so
    /// `REGISTRY_URL` config wins on both fresh and migrated databases).
    pub async fn seed_from_config(
        db: &PgPool,
        seed: &alcedo_common::config::RegistrySeed,
    ) -> Result<bool, AppError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM registries")
            .fetch_one(db)
            .await?;

        if count > 1 {
            return Ok(false);
        }

        // A lone auto-created placeholder is upgraded with the configured
        // values (preserving its id so existing plugin FKs keep resolving).
        if count == 1 {
            let all = Self::find_all(db).await?;
            if let Some(reg) = all.first() {
                if !reg.is_default_placeholder() {
                    return Ok(false);
                }
                Registry::update(
                    db,
                    reg.id,
                    Some(&seed.name),
                    Some(&seed.url),
                    seed.pull_url.as_ref(),
                    Some(&seed.auth_type),
                    seed.username.as_ref(),
                    seed.password.as_ref(),
                )
                .await?;
                return Ok(true);
            }
            return Ok(false);
        }

        let now = chrono::Utc::now();
        Registry::insert(
            db,
            &Registry {
                id: 0,
                name: seed.name.clone(),
                url: seed.url.clone(),
                pull_url: seed.pull_url.clone(),
                auth_type: seed.auth_type.clone(),
                username: seed.username.clone(),
                password: seed.password.clone(),
                created_at: Some(now),
                updated_at: Some(now),
            },
        )
        .await?;

        Ok(true)
    }
}
