use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::ddl::quote;
use crate::services::collections::schema::{SchemaService, TableBuilderExt};
use crate::services::context::AppContext;

/// Plugin request/host-call logging, app-scoped: both tables live in every
/// per-app-version schema so they are dropped with the schema when an app or
/// version is removed. Ported from the v1 system-schema tables
/// `alcedocore_request_logs` and `alcedocore_host_calls`.
pub(crate) struct M0020Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0020Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = SchemaService::new(&state, &self.app_context);

        table_service
            .create_table(
                "plugin_request_logs",
                |builder| {
                    builder.add_col("id", |mut c| {
                        c.not_null()
                            .big_integer()
                            .auto_increment()
                            .primary_key()
                            .clone()
                    });
                    builder.add_col("request_id", |mut c| {
                        c.not_null().string_len(255).clone()
                    });
                    builder.add_col("install_id", |mut c| c.not_null().big_integer().clone());
                    builder.add_col("plugin_slug", |mut c| {
                        c.not_null().string_len(255).clone()
                    });
                    builder.add_col("method", |mut c| c.not_null().string_len(10).clone());
                    builder.add_col("path", |mut c| c.not_null().string_len(2048).clone());
                    builder.add_col("status_code", |mut c| c.not_null().integer().clone());
                    builder.add_col("duration_ms", |mut c| c.not_null().integer().clone());
                    builder.add_col("created_at", |mut c| {
                        c.not_null()
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp())
                            .clone()
                    });
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .create_table(
                "plugin_host_calls",
                |builder| {
                    builder.add_col("id", |mut c| {
                        c.not_null()
                            .big_integer()
                            .auto_increment()
                            .primary_key()
                            .clone()
                    });
                    builder.add_col("parent_request_id", |mut c| {
                        c.not_null().string_len(255).clone()
                    });
                    builder.add_col("plugin_slug", |mut c| {
                        c.not_null().string_len(255).clone()
                    });
                    builder.add_col("action_type", |mut c| {
                        c.not_null().string_len(255).clone()
                    });
                    builder.add_col("args_summary", |mut c| {
                        c.not_null().text().default("").clone()
                    });
                    builder.add_col("result_summary", |mut c| {
                        c.not_null().text().default("").clone()
                    });
                    builder.add_col("duration_ms", |mut c| {
                        c.not_null().integer().default(0).clone()
                    });
                    builder.add_col("created_at", |mut c| {
                        c.not_null()
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp())
                            .clone()
                    });
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        let schema = quote(&self.app_context.schema_name());
        for (name, table, cols) in [
            ("idx_plugin_request_logs_slug", "plugin_request_logs", "plugin_slug"),
            (
                "idx_plugin_request_logs_request_id",
                "plugin_request_logs",
                "request_id",
            ),
            (
                "idx_plugin_request_logs_created",
                "plugin_request_logs",
                "created_at DESC",
            ),
            (
                "idx_plugin_host_calls_parent",
                "plugin_host_calls",
                "parent_request_id",
            ),
            (
                "idx_plugin_host_calls_created",
                "plugin_host_calls",
                "created_at DESC",
            ),
        ] {
            sqlx::query(&format!(
                r#"CREATE INDEX IF NOT EXISTS "{name}" ON {schema}."{table}" ({cols});"#
            ))
            .execute(&mut *connection)
            .await?;
        }

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        for table in ["plugin_host_calls", "plugin_request_logs"] {
            sqlx::query(&format!(
                r#"DROP TABLE IF EXISTS {schema}."{table}" CASCADE;"#
            ))
            .execute(&mut *connection)
            .await?;
        }
        Ok(())
    }
}

pub(crate) struct M0020Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0020Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0020_plugin_logging"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m0019_plugin_policies::M0019Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0020Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
