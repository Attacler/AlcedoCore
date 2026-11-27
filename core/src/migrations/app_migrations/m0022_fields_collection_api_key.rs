use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::services::collections::ddl::quote;
use crate::services::context::AppContext;

/// Unique field api_name per collection, in every app-version schema: two
/// fields of the same collection cannot share an api_name.
pub(crate) struct M0022Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0022Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        // Existing databases may already contain duplicate (collection_id,
        // api_name) rows (e.g. from older clone/import code paths); the unique
        // index cannot be created until they are merged. Keep the lowest id
        // per group and re-point view-field references at it first, otherwise
        // the DELETE would violate the collection_view_fields FK. The
        // view-fields table may legitimately be absent (schemas cloned before
        // m0002 created it), so guard the UPDATE on its existence.
        let view_fields_exists: Option<(bool,)> = sqlx::query_as(
            "SELECT EXISTS (
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = $1 AND table_name = 'alcedocore_collection_view_fields'
            )",
        )
        .bind(self.app_context.schema_name())
        .fetch_optional(&mut *connection)
        .await?;
        if view_fields_exists.map(|(exists,)| exists).unwrap_or(false) {
            sqlx::query(&format!(
                r#"UPDATE {schema}."alcedocore_collection_view_fields" AS cvf
               SET field = k.keep_id
               FROM {schema}."alcedocore_fields" AS f
               JOIN (
                   SELECT collection_id, api_name, MIN(id) AS keep_id
                   FROM {schema}."alcedocore_fields"
                   GROUP BY collection_id, api_name
               ) AS k ON k.collection_id = f.collection_id AND k.api_name = f.api_name
               WHERE cvf.field = f.id AND f.id <> k.keep_id;"#
            ))
            .execute(&mut *connection)
            .await?;
        }
        sqlx::query(&format!(
            r#"DELETE FROM {schema}."alcedocore_fields" AS f
               USING (
                   SELECT collection_id, api_name, MIN(id) AS keep_id
                   FROM {schema}."alcedocore_fields"
                   GROUP BY collection_id, api_name
               ) AS k
               WHERE f.collection_id = k.collection_id
                 AND f.api_name = k.api_name
                 AND f.id <> k.keep_id;"#
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            r#"CREATE UNIQUE INDEX IF NOT EXISTS alcedocore_fields_collection_api_key ON {schema}."alcedocore_fields" (collection_id, api_name);"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"DROP INDEX IF EXISTS {schema}.alcedocore_fields_collection_api_key;"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0022Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0022Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0022_fields_collection_api_key"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m0021_saved_views::M0021Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0022Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
