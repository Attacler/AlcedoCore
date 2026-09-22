use futures::FutureExt;
use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::context::AppContext;
use crate::services::postgres::tables::TableBuilderExt;
use crate::services::postgres::tables::TableService;

pub(crate) struct M0005Operation {
    pub(crate) app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0005Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        state
            .db_new_transaction(|tx| {
                let state = state.clone();
                let app_context = self.app_context.clone();
                async move {
                    let table_service = TableService::new(&state, &app_context);

                    table_service
                        .create_table(
                            "alcedo_developer_api_keys",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null().uuid().primary_key().clone()
                                });
                                builder.add_col("name", |mut c| c.not_null().string().clone());
                                builder.add_col("version_id", |mut c| c.not_null().integer().clone());
                                builder.add_fk(
                                    &app_context,
                                    "alcedo_developer_api_keys",
                                    "version_id",
                                    &app_context,
                                    "alcedo_versions",
                                    "id",
                                );
                                builder.add_col("key_hash", |mut c| c.not_null().text().clone());
                                builder.add_col("key_prefix", |mut c| c.not_null().string().clone());
                                builder.add_col("is_active", |mut c| {
                                    c.not_null().boolean().default(true).clone()
                                });
                                builder.add_col("created_at", |mut c| {
                                    c.date_time().default(Expr::current_timestamp()).clone()
                                });
                                builder.add_col("last_used_at", |mut c| c.date_time().clone());
                            },
                            None,
                            &mut Some(tx),
                        )
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
        sqlx::query("DROP TABLE IF EXISTS alcedo_developer_api_keys CASCADE;")
            .execute(connection)
            .await
            .unwrap();
        Ok(())
    }
}

pub(crate) struct M0005Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0005Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0005_developer_keys"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0005Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
