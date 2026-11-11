use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::ddl::quote;
use crate::services::collections::schema::{SchemaService, TableBuilderExt};
use crate::services::context::AppContext;

/// Menu builder tables (menus, sections, items, role grants) in the app schema.
/// All child tables cascade from their parents so deleting a menu removes the
/// whole tree.
pub(crate) struct M0014Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0014Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = SchemaService::new(&state, &self.app_context);

        table_service
            .create_table(
                "alcedocore_menus",
                |builder| {
                    builder.add_col("id", |mut c| c.not_null().uuid().primary_key().clone());
                    builder.add_col("name", |mut c| c.not_null().string().clone());
                    builder.add_col("icon", |mut c| c.string().default("menu").clone());
                    builder.add_col("created_at", |mut c| {
                        c.date_time().default(Expr::current_timestamp()).clone()
                    });
                    builder.add_col("updated_at", |mut c| {
                        c.date_time().default(Expr::current_timestamp()).clone()
                    });
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .create_table(
                "alcedocore_menu_sections",
                |builder| {
                    builder.add_col("id", |mut c| c.not_null().uuid().primary_key().clone());
                    builder.add_col("menu_id", |mut c| c.not_null().uuid().clone());
                    builder.add_col("label", |mut c| c.not_null().string().clone());
                    builder.add_col("icon", |mut c| c.string().default("").clone());
                    builder.add_col("visible", |mut c| {
                        c.not_null().boolean().default(true).clone()
                    });
                    builder.add_col("sort_order", |mut c| {
                        c.not_null().integer().default(0).clone()
                    });
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .create_table(
                "alcedocore_menu_items",
                |builder| {
                    builder.add_col("id", |mut c| c.not_null().uuid().primary_key().clone());
                    builder.add_col("section_id", |mut c| c.not_null().uuid().clone());
                    builder.add_col("parent_item_id", |mut c| c.uuid().clone());
                    builder.add_col("label", |mut c| c.not_null().string().clone());
                    builder.add_col("icon", |mut c| c.string().default("").clone());
                    builder.add_col("visible", |mut c| {
                        c.not_null().boolean().default(true).clone()
                    });
                    builder.add_col("route", |mut c| c.string().clone());
                    builder.add_col("url", |mut c| c.string().clone());
                    builder.add_col("external", |mut c| {
                        c.not_null().boolean().default(false).clone()
                    });
                    builder.add_col("link_type", |mut c| c.string().default("custom").clone());
                    builder.add_col("sort_order", |mut c| {
                        c.not_null().integer().default(0).clone()
                    });
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .create_table(
                "alcedocore_menu_roles",
                |builder| {
                    builder.add_col("id", |mut c| c.not_null().uuid().primary_key().clone());
                    builder.add_col("menu_id", |mut c| c.not_null().uuid().clone());
                    builder.add_col("role_id", |mut c| c.not_null().uuid().clone());
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        // Foreign keys are added explicitly (the `add_fk` helper does not emit
        // ON DELETE CASCADE) so deleting a menu/section cleans up its children.
        let schema = quote(&self.app_context.schema_name());
        for statement in [
            format!(
                r#"ALTER TABLE {schema}."alcedocore_menu_sections" ADD CONSTRAINT "alcedocore_menu_sections_menu_id_fkey" FOREIGN KEY ("menu_id") REFERENCES {schema}."alcedocore_menus" ("id") ON DELETE CASCADE;"#
            ),
            format!(
                r#"ALTER TABLE {schema}."alcedocore_menu_items" ADD CONSTRAINT "alcedocore_menu_items_section_id_fkey" FOREIGN KEY ("section_id") REFERENCES {schema}."alcedocore_menu_sections" ("id") ON DELETE CASCADE;"#
            ),
            format!(
                r#"ALTER TABLE {schema}."alcedocore_menu_items" ADD CONSTRAINT "alcedocore_menu_items_parent_item_id_fkey" FOREIGN KEY ("parent_item_id") REFERENCES {schema}."alcedocore_menu_items" ("id") ON DELETE CASCADE;"#
            ),
            format!(
                r#"ALTER TABLE {schema}."alcedocore_menu_roles" ADD CONSTRAINT "alcedocore_menu_roles_menu_id_fkey" FOREIGN KEY ("menu_id") REFERENCES {schema}."alcedocore_menus" ("id") ON DELETE CASCADE;"#
            ),
            format!(
                r#"ALTER TABLE {schema}."alcedocore_menu_roles" ADD CONSTRAINT "alcedocore_menu_roles_role_id_fkey" FOREIGN KEY ("role_id") REFERENCES {schema}."alcedocore_roles" ("id") ON DELETE CASCADE;"#
            ),
        ] {
            sqlx::query(&statement).execute(&mut *connection).await?;
        }

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"DROP TABLE IF EXISTS {schema}."alcedocore_menu_roles", {schema}."alcedocore_menu_items", {schema}."alcedocore_menu_sections", {schema}."alcedocore_menus" CASCADE;"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0014Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0014Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0014_menus"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m0013_system_users_fields::M0013Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0014Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
