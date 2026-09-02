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

    /// Insert a registry from env config on startup, but only when the
    /// registries table is currently empty (idempotent across restarts).
    pub async fn seed_from_config(
        db: &PgPool,
        seed: &alcedo_common::config::RegistrySeed,
    ) -> Result<bool, AppError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM registries")
            .fetch_one(db)
            .await?;

        if count > 0 {
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
