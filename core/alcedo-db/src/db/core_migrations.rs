use sqlx::PgPool;
use std::fs;
use std::path::PathBuf;

/// Default path for core migration files inside Docker containers.
/// Can be overridden with the `CORE_MIGRATIONS_DIR` environment variable.
const CORE_MIGRATIONS_DIR_DEFAULT: &str = "/app/core-migrations";

/// Known core migration versions for legacy database pre-population.
const LEGACY_MIGRATIONS: &[(&str, &str)] = &[
    ("core-001", "001_create_plugins.up.sql"),
    ("core-002", "002_create_plugin_versions.up.sql"),
    ("core-003", "003_create_schema_migrations.up.sql"),
    ("core-004", "004_create_request_logs.up.sql"),
    ("core-005", "005_create_registries.up.sql"),
    ("core-006", "006_create_collection_definitions.up.sql"),
    ("core-007", "007_create_saved_views.up.sql"),
    ("core-008", "008_create_system_settings.up.sql"),
    ("core-009", "009_add_request_body_capture.up.sql"),
    ("core-010", "010_create_host_calls.up.sql"),
    ("core-011", "011_activity_logs.up.sql"),
    ("core-012", "012_add_registry_fk.up.sql"),
    ("core-013", "013_create_collection_sections.up.sql"),
    ("core-014", "014_add_collection_display_name.up.sql"),
    ("core-015", "015_create_policies.up.sql"),
    ("core-016", "016_permission_action_single.up.sql"),
    ("core-017", "017_plugin_scopes.up.sql"),
    ("core-018", "018_users.up.sql"),
    ("core-019", "019_roles_permissions.up.sql"),
    (
        "core-020",
        "020_rename_role_permissions_to_role_scopes.up.sql",
    ),
    ("core-021", "021_role_policies.up.sql"),
    ("core-022", "022_update_scope_names.up.sql"),
    ("core-023", "023_seed_system_collections.up.sql"),
    ("core-024", "024_seed_users_fields.up.sql"),
    ("core-025", "025_add_request_log_source.up.sql"),
    ("core-026", "026_create_developer_api_keys.up.sql"),
    ("core-027", "027_create_collection_fields.up.sql"),
    ("core-028", "028_event_subscriptions.up.sql"),
    ("core-029", "029_add_request_id_to_logs.up.sql"),
    ("core-030", "030_add_actor_to_system_logs.up.sql"),
    ("core-031", "031_add_registry_pull_url.up.sql"),
    ("core-032", "032_create_file_metadata.up.sql"),
    ("core-033", "033_create_item_files.up.sql"),
    ("core-034", "034_menus.up.sql"),
    ("core-035", "035_create_plugin_recovery.up.sql"),
    ("core-036", "036_add_last_login_at.up.sql"),
    ("core-037", "037_add_dev_key_prefix_index.up.sql"),
    ("core-038", "038_add_sections_fk.up.sql"),
];

/// A runner for core (application-level) migrations stored in `core-migrations/`.
///
/// Unlike plugin migrations (which live in per-plugin schemas), core migrations
/// operate on the public schema and are tracked in the `schema_migrations` table
/// with version prefixes like `core-001`, `core-002`, etc.
///
/// # Backward Compatibility
///
/// On legacy databases where tables were created by `db-init/*.sql` (via PostgreSQL's
/// `/docker-entrypoint-initdb.d` mechanism), no `core-*` entries exist in the
/// `schema_migrations` table. The runner detects this state and pre-populates all
/// known core migrations as already applied, preserving existing data without
/// attempting to re-create tables.
#[derive(Debug, Clone)]
pub struct CoreMigrationRunner {
    pool: PgPool,
}

impl CoreMigrationRunner {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Ensure the `schema_migrations` tracking table exists (bootstrap).
    async fn ensure_schema_migrations_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS schema_migrations (
                version VARCHAR(100) PRIMARY KEY,
                applied_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                description TEXT
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_schema_migrations_applied_at
               ON schema_migrations(applied_at)"#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Check whether we are running against a database that already has tables
    /// created by the legacy `db-init/*.sql` system (the `plugins` table acts as
    /// the canary).
    async fn is_legacy_database(&self) -> Result<bool, sqlx::Error> {
        let row: (bool,) = sqlx::query_as(
            r#"SELECT EXISTS(
                SELECT 1 FROM information_schema.tables
                WHERE table_name = 'plugins' AND table_schema = 'public'
            )"#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// Return all already-applied core migration versions.
    async fn get_applied_core_migrations(&self) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"SELECT version FROM schema_migrations
               WHERE version LIKE 'core-%'
               ORDER BY applied_at ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(v,)| v).collect())
    }

    /// Record a single migration as applied, skipping duplicates safely.
    async fn record_migration(&self, version: &str, description: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO schema_migrations (version, description) VALUES ($1, $2)
               ON CONFLICT (version) DO NOTHING"#,
        )
        .bind(version)
        .bind(description)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Pre-populate the `schema_migrations` table for a legacy database where
    /// tables already exist (from `db-init/*.sql`) but no `core-*` tracking
    /// entries are present.
    async fn pre_populate_legacy_migrations(&self) -> Result<Vec<String>, sqlx::Error> {
        let mut recorded = Vec::new();
        for (version, description) in LEGACY_MIGRATIONS {
            self.record_migration(version, description).await?;
            recorded.push(version.to_string());
        }
        tracing::info!(
            "Pre-populated {} legacy core migration(s) for existing database",
            recorded.len()
        );
        Ok(recorded)
    }

    /// Run all pending core migrations.
    ///
    /// Returns a list of versions that were applied (or `["legacy-pre-populated"]`
    /// when an existing legacy database was detected).
    pub async fn run_pending(
        &self,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        // 1. Bootstrap the schema_migrations tracking table.
        self.ensure_schema_migrations_table().await?;

        // 2. Detect database state.
        let applied = self.get_applied_core_migrations().await?;
        let applied_set: std::collections::HashSet<String> = applied.into_iter().collect();

        // 3. Legacy DB handling — tables exist but no core-* entries.
        if applied_set.is_empty() && self.is_legacy_database().await? {
            let pre_populated = self.pre_populate_legacy_migrations().await?;
            return Ok(pre_populated);
        }

        // 4. Resolve the core-migrations directory path.
        let migrations_dir = std::env::var("CORE_MIGRATIONS_DIR")
            .unwrap_or_else(|_| CORE_MIGRATIONS_DIR_DEFAULT.to_string());
        let dir = PathBuf::from(&migrations_dir);

        if !dir.exists() {
            tracing::warn!(
                "Core migrations directory '{}' not found. Skipping core migrations.",
                migrations_dir
            );
            panic!("Could not resolve migrations");
        }

        // 5. Collect .up.sql files sorted by filename.
        let mut entries: Vec<_> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().map_or(false, |e| e == "sql")
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .map_or(false, |s| s.ends_with(".up"))
            })
            .collect();

        entries.sort_by(|a, b| {
            let name_a = a.file_name().unwrap_or_default().to_string_lossy();
            let name_b = b.file_name().unwrap_or_default().to_string_lossy();
            name_a.cmp(&name_b)
        });

        // 6. Apply unapplied migrations in order.
        let mut executed = Vec::new();

        for path in &entries {
            let filename = path.file_name().unwrap().to_str().unwrap();

            // Extract numeric portion: e.g. "001" from "001_create_plugins.up.sql"
            let version_num = filename.split('_').next().unwrap_or("");
            let version = format!("core-{}", version_num);

            if applied_set.contains(&version) {
                tracing::debug!("Core migration already applied: {}", version);
                continue;
            }

            let description = filename.strip_suffix(".up.sql").unwrap_or(filename);

            tracing::info!("Applying core migration {} ({})...", version, filename);
            let content = fs::read_to_string(path)?;

            // Execute each statement separately (split by semicolons,
            // respecting dollar-quoted strings and comments).
            let statements = super::split_sql_statements(&content);

            for stmt in &statements {
                sqlx::query(stmt).execute(&self.pool).await?;
            }

            self.record_migration(&version, description).await?;
            executed.push(format!("{} ({})", version, filename));
            tracing::info!("Applied core migration: {} ({})", version, filename);
        }

        Ok(executed)
    }
}
