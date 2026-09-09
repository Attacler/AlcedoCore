use alcedo_common::context::AppContext;
use sea_query::{Alias, ColumnDef, ForeignKey, TableCreateStatement};
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::core_state_for_migrations_from_env;
use crate::services::tables::TableService;

/// The `CoreMigrationRunner` (SQL files, already wired into both bins)
/// owns the `alcedo.*` source tables with UUID keys. This sqlx_migrator
/// operation carries the prototype's integer-key DDL, so it must never
/// recreate or reshape those tables — only create them when missing
/// (e.g. databases bootstrapped purely through this runner).
async fn table_exists(pool: &sqlx::PgPool, schema: &str, table: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("{}.{}", schema, table))
        .fetch_one(pool)
        .await
        .unwrap_or(false)
}
pub(crate) struct M0001Operation {
    app_context: AppContext,
}

trait TableBuilderExt {
    fn add_col(&mut self, name: &str, f: impl FnOnce(ColumnDef) -> ColumnDef) -> &mut Self;
    fn add_fk(
        &mut self,
        from_context: &AppContext,
        from_table: &str,
        from_col: &str,
        to_context: &AppContext,
        to_table: &str,
        to_col: &str,
    ) -> &mut Self;
}

impl TableBuilderExt for TableCreateStatement {
    fn add_col(&mut self, name: &str, f: impl FnOnce(ColumnDef) -> ColumnDef) -> &mut Self {
        self.col(f(ColumnDef::new(Alias::new(name))));
        self
    }

    fn add_fk(
        &mut self,
        from_context: &AppContext,
        from_table: &str,
        from_col: &str,
        _to_context: &AppContext,
        to_table: &str,
        to_col: &str,
    ) -> &mut Self {
        self.foreign_key(
            ForeignKey::create()
                .name(format!(
                    "{}_{}_{}_{}",
                    from_table, from_col, to_table, to_col
                ))
                .from(
                    (
                        Alias::new(from_context.schema_name()),
                        Alias::new(from_table),
                    ),
                    Alias::new(from_col),
                )
                .to(
                    (Alias::new(from_context.schema_name()), Alias::new(to_table)),
                    Alias::new(to_col),
                ),
        );
        self
    }
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0001Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        // NOTE: the prototype built a full plugin-layer `AppState` here via
        // `generate_app_state_for_migrations()` (which no longer exists, and
        // would be a dependency cycle). Migrations only need the core state.
        let core = core_state_for_migrations_from_env().await;
        let pool = core
            .pool()
            .map_err(|e| Error::Box(Box::new(e)))?
            .clone();
        let table_service = TableService::new(&core, &self.app_context);
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;
        let mut tx_ref = Some(&mut tx);

        if !table_exists(&pool, "alcedo", "alcedo_apps").await {
                    table_service
                        .create_table(
                            "alcedo_apps",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null()
                                        .integer()
                                        .auto_increment()
                                        .primary_key()
                                        .clone()
                                });
                                builder.add_col("name", |mut c| c.not_null().string().clone());
                                builder.add_col("api_name", |mut c| c.not_null().string().clone());
                                builder.add_col("icon", |mut c| c.string().clone());
                                builder.add_col("logo", |mut c| c.string().clone());
                            },
                            None,
                            &mut tx_ref,
                        )
                        .await
                        .map_err(|e| Error::Box(Box::new(e)))?;
        }

        if !table_exists(&pool, "alcedo", "alcedo_versions").await {
                    table_service
                        .create_table(
                            "alcedo_versions",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null()
                                        .integer()
                                        .auto_increment()
                                        .primary_key()
                                        .clone()
                                });
                                builder
                                    .add_col("version_name", |mut c| c.not_null().string().clone());
                            },
                            None,
                            &mut tx_ref,
                        )
                        .await
                        .map_err(|e| Error::Box(Box::new(e)))?;
        }

        if !table_exists(&pool, "alcedo", "alcedo_apps_versions").await {
                    table_service
                        .create_table(
                            "alcedo_apps_versions",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null()
                                        .integer()
                                        .auto_increment()
                                        .primary_key()
                                        .clone()
                                });
                                builder.add_col("app_id", |mut c| c.not_null().integer().clone());
                                builder.add_fk(
                                    &self.app_context,
                                    "alcedo_apps_versions",
                                    "app_id",
                                    &self.app_context,
                                    "alcedo_apps",
                                    "id",
                                );
                                builder
                                    .add_col("version_id", |mut c| c.not_null().integer().clone());
                                builder.add_fk(
                                    &self.app_context,
                                    "alcedo_apps_versions",
                                    "version_id",
                                    &self.app_context,
                                    "alcedo_versions",
                                    "id",
                                );
                            },
                            None,
                            &mut tx_ref,
                        )
                        .await
                        .map_err(|e| Error::Box(Box::new(e)))?;
        }

        tx.commit()
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query("DROP SCHEMA IF EXISTS alcedo CASCADE;")
            .execute(connection)
            .await
            .unwrap();
        Ok(())
    }
}

pub(crate) struct M0001Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0001Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0001_init"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0001Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
