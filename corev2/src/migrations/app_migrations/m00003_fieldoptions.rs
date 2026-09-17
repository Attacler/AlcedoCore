use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::context::AppContext;
use crate::services::postgres::tables::{FieldCreationObject, TableService};

pub(crate) struct M0003Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0003Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let app_context = self.app_context.clone();

        let table_service = TableService::new(&state, &app_context);

        table_service
            .add_field(
                "alcedo_fields",
                FieldCreationObject {
                    name: "options".to_string(),
                    col_type: "JSONB".to_string(),
                    is_nullable: Some(true),
                    ..Default::default()
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        Ok(())
    }

    async fn down(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = TableService::new(&state, &self.app_context);
        table_service
            .drop_field("alcedo_fields", "options", &mut None)
            .await
            .map_err(|e: crate::services::errors::AlcedoError| Error::Box(Box::new(e)))?;
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
        "m0003_fieldoptions"
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
