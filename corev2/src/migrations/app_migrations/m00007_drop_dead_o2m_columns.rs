use sqlx::{PgConnection, Postgres, Row};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::services::collections::ddl::{quote, related_app_schema};
use crate::services::context::AppContext;

pub(crate) struct M0007Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0007Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());

        let rows = sqlx::query(&format!(
            "SELECT c.\"table\" AS source_table, f.api_name AS field_name, \
                    f.options->>'related_collection' AS related_collection, \
                    f.options->>'related_app' AS related_app \
             FROM {schema}.alcedo_fields f \
             JOIN {schema}.alcedo_collections c ON c.id = f.collection_id \
             WHERE f.options->>'type' = 'relationship' \
               AND f.options->>'relationship_type' = 'one_to_many'"
        ))
        .fetch_all(&mut *connection)
        .await?;

        for row in rows {
            let source: String = row.get("source_table");
            let field: String = row.get("field_name");
            let related: Option<String> = row.get("related_collection");
            let related_app: Option<String> = row.get("related_app");

            let related = match related {
                Some(r) if !r.is_empty() => r,
                _ => continue,
            };
            let child_schema = related_app_schema(&self.app_context, &related_app);

            let exists: Option<i32> = sqlx::query_scalar(
                "SELECT 1 FROM information_schema.tables \
                 WHERE table_schema = $1 AND table_name = $2",
            )
            .bind(&child_schema)
            .bind(&related)
            .fetch_optional(&mut *connection)
            .await?;
            if exists.is_none() {
                continue;
            }

            let column = format!("{}_{}_id", source, field);
            let sql = format!(
                "ALTER TABLE {}.{} DROP COLUMN IF EXISTS {}",
                quote(&child_schema),
                quote(&related),
                quote(&column)
            );
            sqlx::query(&sql).execute(&mut *connection).await?;
        }

        Ok(())
    }

    async fn down(&self, _connection: &mut PgConnection) -> Result<(), Error> {
        Ok(())
    }
}

pub(crate) struct M0007Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0007Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0007_drop_dead_o2m_columns"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m00006_section_related_app::M0006Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0007Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
