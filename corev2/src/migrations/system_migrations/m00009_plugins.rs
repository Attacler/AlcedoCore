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

/// Plugin installs.
///
/// - `alcedocore_plugins` is the **catalog**: plugin identity (slug, type,
///   registry, endpoints, requested scopes). No per-install state.
/// - `alcedocore_plugins_installs` is the **install**: a plugin deployed to a
///   specific **app × version** pair (`alcedocore_apps_versions`), carrying the
///   deployed image tag, settings, enabled flag and granted scopes. Cascades
///   with the plugin and with the app×version link.
pub(crate) struct M0009Operation {
    pub(crate) app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0009Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        state
            .db_new_transaction(|tx| {
                let state = state.clone();
                let app_context = self.app_context.clone();
                async move {
                    let table_service = SchemaService::new(&state, &app_context);
                    let schema = app_context.schema_name();

                    // Catalog.
                    table_service
                        .create_table(
                            "alcedocore_plugins",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null()
                                        .integer()
                                        .auto_increment()
                                        .primary_key()
                                        .clone()
                                });
                                builder.add_col("slug", |mut c| c.not_null().string().clone());
                                builder.add_col("plugin_type", |mut c| {
                                    c.not_null()
                                        .string()
                                        .default(Expr::value("dynamic"))
                                        .clone()
                                });
                                // Always set — plugins are pulled from a registry.
                                builder.add_col("registry_id", |mut c| {
                                    c.not_null().integer().clone()
                                });
                                builder.add_col("description", |mut c| c.text().clone());
                                builder.add_col("endpoints", |mut c| {
                                    c.json_binary().not_null().default("{}").clone()
                                });
                                builder.add_col("documentation", |mut c| {
                                    c.json_binary().not_null().default("[]").clone()
                                });
                                builder.add_col("requested_scopes", |mut c| {
                                    c.json_binary().not_null().default("[]").clone()
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
                            },
                            None,
                            &mut Some(tx),
                        )
                        .await?;

                    sqlx::query(
                        "CREATE UNIQUE INDEX IF NOT EXISTS uq_alcedocore_plugins_slug \
                         ON alcedocore.alcedocore_plugins(slug)",
                    )
                    .execute(&mut **tx)
                    .await?;

                    // Installs (plugin × app-version).
                    table_service
                        .create_table(
                            "alcedocore_plugins_installs",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null()
                                        .integer()
                                        .auto_increment()
                                        .primary_key()
                                        .clone()
                                });
                                builder.add_col("plugin_id", |mut c| {
                                    c.not_null().integer().clone()
                                });
                                builder.add_col("app_version_id", |mut c| {
                                    c.not_null().integer().clone()
                                });
                                builder.add_col("plugin_version", |mut c| {
                                    c.not_null().string().clone()
                                });
                                builder.add_col("enabled", |mut c| {
                                    c.not_null().boolean().default(Expr::value(false)).clone()
                                });
                                builder.add_col("settings", |mut c| {
                                    c.json_binary().not_null().default("{}").clone()
                                });
                                builder.add_col("granted_scopes", |mut c| {
                                    c.json_binary().not_null().default("[]").clone()
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
                                        .name("alcedocore_plugins_installs_plugin_id_fkey")
                                        .from(
                                            (
                                                Alias::new(&schema),
                                                Alias::new("alcedocore_plugins_installs"),
                                            ),
                                            Alias::new("plugin_id"),
                                        )
                                        .to(
                                            (Alias::new(&schema), Alias::new("alcedocore_plugins")),
                                            Alias::new("id"),
                                        )
                                        .on_delete(ForeignKeyAction::Cascade),
                                );
                                builder.foreign_key(
                                    ForeignKey::create()
                                        .name("alcedocore_plugins_installs_app_version_id_fkey")
                                        .from(
                                            (
                                                Alias::new(&schema),
                                                Alias::new("alcedocore_plugins_installs"),
                                            ),
                                            Alias::new("app_version_id"),
                                        )
                                        .to(
                                            (
                                                Alias::new(&schema),
                                                Alias::new("alcedocore_apps_versions"),
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

                    sqlx::query(
                        "CREATE UNIQUE INDEX IF NOT EXISTS uq_alcedocore_plugins_installs \
                         ON alcedocore.alcedocore_plugins_installs(plugin_id, app_version_id)",
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
        for table in ["alcedocore_plugins_installs", "alcedocore_plugins"] {
            sqlx::query(&format!(
                "DROP TABLE IF EXISTS alcedocore.{table} CASCADE;"
            ))
            .execute(&mut *connection)
            .await
            .unwrap();
        }
        Ok(())
    }
}

pub(crate) struct M0009Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0009Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0009_plugins"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0009Operation {
            app_context: self.app_context.clone(),
        })]
    }
}