use futures::FutureExt;
use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::item_map;
use crate::migrations::generate_app_state_for_migrations;
use crate::services::context::AppContext;
use crate::services::items::service::ItemsService;
use crate::services::collections::schema::TableBuilderExt;
use crate::services::collections::schema::SchemaService;
pub(crate) struct M0003Operation {
    pub(crate) app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0003Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        state
            .db_new_transaction(|tx| {
                let state = state.clone();
                let app_context = self.app_context.clone();
                async move {
                    let table_service = SchemaService::new(&state, &app_context);

                    let table_name = "alcedo_settings".to_string();
                    table_service
                        .create_table(
                            &table_name,
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null()
                                        .uuid()
                                        .primary_key()
                                        .default(Expr::cust("gen_random_uuid()"))
                                        .clone()
                                });
                                builder.add_col("platform_name", |mut c| c.string().clone());
                            },
                            None,
                            &mut None,
                        )
                        .await?;

                    let items_service = ItemsService::new(&state, &app_context, &table_name);

                    items_service
                        .create_many(
                            vec![item_map! {
                                "platform_name" => "AlcedoCore",
                            }],
                            &mut None,
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
        sqlx::query("DROP TABLE IF EXISTS alcedo_settings CASCADE;")
            .execute(connection)
            .await
            .unwrap();
        Ok(())
    }
}

pub(crate) struct M0003Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0003Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0003_settings"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0003Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
