use futures::FutureExt;
use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::schema::SchemaService;
use crate::services::collections::schema::TableBuilderExt;
use crate::services::context::AppContext;

pub(crate) struct M0004Operation {
    pub(crate) app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0004Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        state
            .db_new_transaction(|tx| {
                let state = state.clone();
                let app_context = self.app_context.clone();
                async move {
                    let table_service = SchemaService::new(&state, &app_context);

                    table_service
                        .create_table(
                            "alcedo_sessions",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null().uuid().primary_key().clone()
                                });
                                builder.add_col("user_id", |mut c| c.not_null().uuid().clone());
                                builder.add_fk(
                                    &app_context,
                                    "alcedo_sessions",
                                    "user_id",
                                    &app_context,
                                    "alcedo_users",
                                    "id",
                                );
                                builder.add_col("user_agent", |mut c| c.string().clone());
                                builder.add_col("created_at", |mut c| {
                                    c.date_time().default(Expr::current_timestamp()).clone()
                                });
                                builder.add_col("expires_at", |mut c| {
                                    c.date_time().not_null().clone()
                                });
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
        sqlx::query("DROP TABLE IF EXISTS alcedo_sessions CASCADE;")
            .execute(connection)
            .await
            .unwrap();
        Ok(())
    }
}

pub(crate) struct M0004Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0004Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0004_sessions"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0004Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
