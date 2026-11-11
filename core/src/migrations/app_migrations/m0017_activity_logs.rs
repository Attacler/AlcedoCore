use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::ddl::quote;
use crate::services::collections::schema::{SchemaService, TableBuilderExt};
use crate::services::context::AppContext;

/// App-scoped activity logs: system logs (audit/admin actions) and collection
/// logs (item CRUD history), created in every per-app-version schema.
pub(crate) struct M0017Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0017Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = SchemaService::new(&state, &self.app_context);

        table_service
            .create_table(
                "alcedocore_system_logs",
                |builder| {
                    builder.add_col("id", |mut c| {
                        c.not_null()
                            .big_integer()
                            .auto_increment()
                            .primary_key()
                            .clone()
                    });
                    builder.add_col("actor_id", |mut c| c.uuid().clone());
                    builder.add_col("action", |mut c| c.not_null().string_len(255).clone());
                    builder.add_col("target", |mut c| c.not_null().string_len(500).clone());
                    builder.add_col("description", |mut c| c.text().clone());
                    builder.add_col("metadata", |mut c| {
                        c.json_binary().not_null().default("{}").clone()
                    });
                    builder.add_col("request_id", |mut c| c.string_len(36).clone());
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
                "alcedocore_collection_logs",
                |builder| {
                    builder.add_col("id", |mut c| {
                        c.not_null()
                            .big_integer()
                            .auto_increment()
                            .primary_key()
                            .clone()
                    });
                    builder.add_col("action", |mut c| c.not_null().string_len(255).clone());
                    builder.add_col("collection_name", |mut c| {
                        c.not_null().string_len(255).clone()
                    });
                    builder.add_col("item_id", |mut c| c.json_binary().not_null().clone());
                    builder.add_col("diff", |mut c| c.json_binary().clone());
                    builder.add_col("metadata", |mut c| {
                        c.json_binary().not_null().default("{}").clone()
                    });
                    builder.add_col("request_id", |mut c| c.string_len(36).clone());
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

        // Indexes mirroring v1's `alcedocore_system_logs` indexes. Index names
        // are intentionally NOT schema-qualified: `CREATE INDEX` rejects a
        // qualified index name and creates the index in the table's schema.
        let schema = quote(&self.app_context.schema_name());
        for (name, cols) in [
            ("idx_alcedocore_system_logs_created_at", "created_at DESC"),
            ("idx_alcedocore_system_logs_action", "action"),
            ("idx_alcedocore_system_logs_target", "target"),
            ("idx_alcedocore_system_logs_request_id", "request_id"),
            ("idx_alcedocore_system_logs_actor_id", "actor_id"),
        ] {
            sqlx::query(&format!(
                r#"CREATE INDEX IF NOT EXISTS "{name}" ON {schema}."alcedocore_system_logs" ({cols});"#
            ))
            .execute(&mut *connection)
            .await?;
        }

        // Indexes mirroring v1's `alcedocore_collection_logs` indexes; `item_id`
        // gets a GIN index (`USING gin` goes before the column list).
        for (name, cols) in [
            ("idx_alcedocore_collection_logs_created_at", "created_at DESC"),
            ("idx_alcedocore_collection_logs_action", "action"),
            ("idx_alcedocore_collection_logs_collection_name", "collection_name"),
            ("idx_alcedocore_collection_logs_request_id", "request_id"),
        ] {
            sqlx::query(&format!(
                r#"CREATE INDEX IF NOT EXISTS "{name}" ON {schema}."alcedocore_collection_logs" ({cols});"#
            ))
            .execute(&mut *connection)
            .await?;
        }
        sqlx::query(&format!(
            r#"CREATE INDEX IF NOT EXISTS "idx_alcedocore_collection_logs_item_id" ON {schema}."alcedocore_collection_logs" USING gin(item_id);"#
        ))
        .execute(&mut *connection)
        .await?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        for table in ["alcedocore_collection_logs", "alcedocore_system_logs"] {
            sqlx::query(&format!(
                r#"DROP TABLE IF EXISTS {schema}."{table}" CASCADE;"#
            ))
            .execute(&mut *connection)
            .await?;
        }
        Ok(())
    }
}

pub(crate) struct M0017Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0017Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0017_activity_logs"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m0016_files::M0016Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0017Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
