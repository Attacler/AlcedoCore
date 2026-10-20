use futures::FutureExt;
use sea_query::{Alias, Expr, ForeignKey, ForeignKeyAction};
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::schema::SchemaService;
use crate::services::collections::schema::TableBuilderExt;
use crate::services::context::AppContext;

/// Route mappings: vanity (domain, path-prefix) → plugin install.
///
/// `alcedocore_route_mappings` is the authoritative store. The active set is
/// mirrored into the shared cache (Redis or in-memory) so every replica can
/// resolve aliases without a DB round-trip per request.
pub(crate) struct M0012Operation {
    pub(crate) app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0012Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        state
            .db_new_transaction(|tx| {
                let state = state.clone();
                let app_context = self.app_context.clone();
                async move {
                    let table_service = SchemaService::new(&state, &app_context);
                    let schema = app_context.schema_name();

                    table_service
                        .create_table(
                            "alcedocore_route_mappings",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null()
                                        .integer()
                                        .auto_increment()
                                        .primary_key()
                                        .clone()
                                });
                                // Lowercase-normalized FQDN; punycode allowed.
                                builder.add_col("domain", |mut c| c.text().clone());
                                // Normalized: leading slash, no trailing slash.
                                builder.add_col("path_prefix", |mut c| c.text().clone());
                                // Target plugin install id.
                                builder.add_col("install_id", |mut c| {
                                    c.not_null().integer().clone()
                                });
                                builder.add_col("is_active", |mut c| {
                                    c.not_null().boolean().default(Expr::value(true)).clone()
                                });
                                builder.add_col("created_at", |mut c| {
                                    c.timestamp_with_time_zone()
                                        .default(Expr::current_timestamp())
                                        .clone()
                                });
                                builder.add_col("updated_at", |mut c| {
                                    c.timestamp_with_time_zone()
                                        .default(Expr::current_timestamp())
                                        .clone()
                                });

                                builder.foreign_key(
                                    ForeignKey::create()
                                        .name("alcedocore_route_mappings_install_id_fkey")
                                        .from(
                                            (
                                                Alias::new(&schema),
                                                Alias::new("alcedocore_route_mappings"),
                                            ),
                                            Alias::new("install_id"),
                                        )
                                        .to(
                                            (
                                                Alias::new(&schema),
                                                Alias::new("alcedocore_plugins_installs"),
                                            ),
                                            Alias::new("id"),
                                        )
                                        .on_delete(ForeignKeyAction::Cascade),
                                );
                            },
                            None,
                            &mut Some(tx),
                        )
                        .await?;

                    // At least one of domain / path_prefix must be set.
                    sqlx::query(
                        "ALTER TABLE alcedocore.alcedocore_route_mappings \
                         ADD CONSTRAINT chk_route_mappings_domain_or_prefix \
                         CHECK (domain IS NOT NULL OR path_prefix IS NOT NULL)",
                    )
                    .execute(&mut **tx)
                    .await?;

                    // Unique on the (domain, path_prefix) pair. NULLs are
                    // distinct in Postgres, so multiple NULL-domain rows with
                    // the same prefix are allowed — the API layer enforces
                    // stricter overlap rules.
                    sqlx::query(
                        "CREATE UNIQUE INDEX IF NOT EXISTS uq_alcedocore_route_mappings \
                         ON alcedocore.alcedocore_route_mappings(domain, path_prefix)",
                    )
                    .execute(&mut **tx)
                    .await?;

                    Ok(())
                }
                .boxed()
            })
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query("DROP TABLE IF EXISTS alcedocore.alcedocore_route_mappings CASCADE;")
            .execute(&mut *connection)
            .await
            .unwrap();
        Ok(())
    }
}

pub(crate) struct M0012Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0012Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0012_route_mappings"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0012Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
