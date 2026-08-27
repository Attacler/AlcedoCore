use sqlx::PgPool;
use std::fs;

const MIGRATIONS_DIR: &str = "db-init";

#[derive(Debug, Clone)]
pub struct MigrationRunner {
    pool: PgPool,
}

impl MigrationRunner {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_applied_migrations(&self) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"SELECT version FROM schema_migrations ORDER BY applied_at ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(v,)| v).collect())
    }

    pub async fn record_migration(&self, version: &str, description: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO schema_migrations (version, description) VALUES ($1, $2)"#,
        )
        .bind(version)
        .bind(description)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn run_pending(&self) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let applied = self.get_applied_migrations().await?;
        let mut applied_set = std::collections::HashSet::new();
        for v in &applied {
            applied_set.insert(v.clone());
        }

        let mut executed = Vec::new();
        let entries = fs::read_dir(MIGRATIONS_DIR)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "sql") {
                let filename = path.file_name().unwrap().to_str().unwrap();
                if filename == "01-schema.sql" {
                    continue;
                }
                let version = filename.split('-').next().unwrap_or(filename).to_string();
                if !applied_set.contains(&version) {
                    println!("Applying migration {}...", filename);
                    let content = fs::read_to_string(&path)?;
                    sqlx::query(&content).execute(&self.pool).await?;
                    self.record_migration(&version, filename).await?;
                    executed.push(filename.to_string());
                    println!("Applied: {}", filename);
                }
            }
        }

        Ok(executed)
    }

    pub async fn rollback_to(&self, target_version: &str) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"SELECT version FROM schema_migrations WHERE version > $1 ORDER BY applied_at DESC"#,
        )
        .bind(target_version)
        .fetch_all(&self.pool)
        .await?;

        let mut rolled_back = Vec::new();
        for (version,) in rows {
            sqlx::query(r#"DELETE FROM schema_migrations WHERE version = $1"#)
                .bind(&version)
                .execute(&self.pool)
                .await?;
            rolled_back.push(version);
        }

        Ok(rolled_back)
    }

    pub async fn get_latest_migration(&self) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"SELECT version FROM schema_migrations ORDER BY applied_at DESC LIMIT 1"#,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(v,)| v))
    }
}