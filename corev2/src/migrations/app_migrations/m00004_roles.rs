use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::schema::SchemaService;
use crate::services::collections::schema::TableBuilderExt;
use crate::services::context::{AppContext, RequestSource};
use futures::FutureExt;

pub(crate) struct M0004Operation {
    app_context: AppContext,
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
                    let global_context = AppContext::system(RequestSource::Migration);

                    table_service
                        .create_table(
                            "alcedo_roles",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null().uuid().primary_key().clone()
                                });
                                builder.add_col("name", |mut c| c.not_null().string().clone());
                                builder.add_col("description", |mut c| c.not_null().text().clone());
                                builder.add_col("is_system", |mut c| {
                                    c.boolean().default(false).clone()
                                });
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

                    table_service
                        .create_table(
                            "alcedo_role_scopes",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null().uuid().primary_key().clone()
                                });
                                builder.add_col("role_id", |mut c| c.not_null().uuid().clone());
                                builder.add_col("scope", |mut c| c.not_null().string().clone());
                            },
                            None,
                            &mut Some(tx),
                        )
                        .await?;

                    table_service
                        .create_table(
                            "alcedo_user_roles",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null().uuid().primary_key().clone()
                                });
                                builder.add_col("user_id", |mut c| c.not_null().uuid().clone());
                                builder.add_col("role_id", |mut c| c.not_null().uuid().clone());
                                builder.add_fk(
                                    &app_context,
                                    "alcedo_user_roles",
                                    "user_id",
                                    &global_context,
                                    "alcedo_users",
                                    "id",
                                );
                                builder.add_fk(
                                    &app_context,
                                    "alcedo_user_roles",
                                    "role_id",
                                    &app_context,
                                    "alcedo_roles",
                                    "id",
                                );
                            },
                            None,
                            &mut Some(tx),
                        )
                        .await?;

                    table_service
                        .create_table(
                            "alcedocore_policies",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null().uuid().primary_key().clone()
                                });
                                builder.add_col("name", |mut c| {
                                    c.not_null().unique_key().string().clone()
                                });
                                builder.add_col("description", |mut c| c.text().clone());
                            },
                            None,
                            &mut Some(tx),
                        )
                        .await?;

                    table_service
                        .create_table(
                            "alcedocore_role_policies",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null().uuid().primary_key().clone()
                                });
                                builder.add_col("role_id", |mut c| c.not_null().uuid().clone());
                                builder.add_col("policy_id", |mut c| c.not_null().uuid().clone());
                                builder.add_fk(
                                    &app_context,
                                    "alcedocore_role_policies",
                                    "role_id",
                                    &app_context,
                                    "alcedo_roles",
                                    "id",
                                );
                            },
                            None,
                            &mut Some(tx),
                        )
                        .await?;

                    table_service
                        .create_table(
                            "alcedocore_policy_permissions",
                            |builder| {
                                builder.add_col("id", |mut c| {
                                    c.not_null().uuid().primary_key().clone()
                                });
                                builder.add_col("policy_id", |mut c| c.not_null().uuid().clone());
                                builder.add_fk(
                                    &app_context,
                                    "alcedocore_policy_permissions",
                                    "policy_id",
                                    &app_context,
                                    "alcedocore_role_policies",
                                    "id",
                                );

                                builder
                                    .add_col("collection", |mut c| c.not_null().integer().clone());
                                builder.add_fk(
                                    &app_context,
                                    "alcedocore_policy_permissions",
                                    "collection",
                                    &app_context,
                                    "alcedo_collections",
                                    "id",
                                );

                                builder.add_col("action", |mut c| c.not_null().string().clone());
                                builder.add_col("fields", |mut c| {
                                    c.not_null().json().default("[]").clone()
                                });
                                builder.add_col("filter", |mut c| {
                                    c.not_null().json().default("[]").clone()
                                });
                                builder.add_col("field_validation", |mut c| {
                                    c.not_null().json().default("[]").clone()
                                });
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
        sqlx::query("DROP TABLE IF EXISTS alcedo_roles,alcedo_role_scopes,alcedo_user_roles,alcedocore_policies,alcedocore_role_policies,alcedocore_policy_permissions CASCADE;")
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
        "M0004_rples"
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
