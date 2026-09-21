use futures::FutureExt;
use sea_query::{Alias, ColumnDef, ForeignKey, TableCreateStatement};
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::context::AppContext;
use crate::services::postgres::tables::TableBuilderExt;
use crate::services::postgres::tables::TableService;
pub(crate) struct M0001Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0001Operation {
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
                            &mut Some(tx),
                        )
                        .await?;

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
                            &mut Some(tx),
                        )
                        .await?;

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
                                    &app_context,
                                    "alcedo_apps_versions",
                                    "app_id",
                                    &app_context,
                                    "alcedo_apps",
                                    "id",
                                );
                                builder
                                    .add_col("version_id", |mut c| c.not_null().integer().clone());
                                builder.add_fk(
                                    &app_context,
                                    "alcedo_apps_versions",
                                    "version_id",
                                    &app_context,
                                    "alcedo_versions",
                                    "id",
                                );
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
