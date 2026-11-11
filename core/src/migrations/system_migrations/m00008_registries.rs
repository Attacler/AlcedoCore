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

pub(crate) struct M0008Operation {
    pub(crate) app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0008Operation {
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
                            "alcedocore_registries",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null()
                                        .integer()
                                        .auto_increment()
                                        .primary_key()
                                        .clone()
                                });
                                builder.add_col("name", |mut c| c.not_null().string().clone());
                                builder.add_col("url", |mut c| c.not_null().string().clone());
                                builder.add_col("pull_url", |mut c| c.string().clone());
                                builder.add_col("auth_type", |mut c| {
                                    c.not_null().string().default(Expr::value("none")).clone()
                                });
                                builder.add_col("username", |mut c| c.string().clone());
                                builder.add_col("password", |mut c| c.text().clone());
                                builder.add_col("created_at", |mut c| {
                                    c.date_time().default(Expr::current_timestamp()).clone()
                                });
                                builder.add_col("updated_at", |mut c| {
                                    c.date_time().default(Expr::current_timestamp()).clone()
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
        sqlx::query("DROP TABLE IF EXISTS alcedocore_registries CASCADE;")
            .execute(connection)
            .await
            .unwrap();
        Ok(())
    }
}

pub(crate) struct M0008Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0008Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0008_registries"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0008Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
