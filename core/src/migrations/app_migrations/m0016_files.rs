use sea_query::{Alias, Expr, Index};
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::ddl::quote;
use crate::services::collections::schema::{SchemaService, TableBuilderExt};
use crate::services::context::{AppContext, RequestSource};

/// File/media tables in the app schema: folders, file metadata and the
/// item↔file link table used for per-record file permissions.
pub(crate) struct M0016Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0016Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = SchemaService::new(&state, &self.app_context);
        let users = AppContext::system(RequestSource::Migration);

        table_service
            .create_table(
                "alcedocore_file_folders",
                |builder| {
                    builder.add_col("id", |mut c| c.not_null().uuid().primary_key().clone());
                    builder.add_col("name", |mut c| c.not_null().string().clone());
                    builder.add_col("parent_id", |mut c| c.uuid().clone());
                    builder.add_col("created_by", |mut c| c.uuid().clone());
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
                "alcedocore_file_metadata",
                |builder| {
                    builder.add_col("id", |mut c| c.not_null().uuid().primary_key().clone());
                    builder.add_col("filename", |mut c| c.not_null().string().clone());
                    builder.add_col("mime_type", |mut c| c.not_null().string().clone());
                    builder.add_col("size_bytes", |mut c| c.not_null().big_integer().clone());
                    builder.add_col("storage_provider", |mut c| c.not_null().string().clone());
                    builder.add_col("storage_path", |mut c| c.not_null().string().clone());
                    builder.add_col("sha256", |mut c| c.string().clone());
                    builder.add_col("alt_text", |mut c| c.string().clone());
                    builder.add_col("folder_id", |mut c| c.uuid().clone());
                    builder.add_col("uploaded_by", |mut c| c.uuid().clone());
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
                "alcedocore_item_files",
                |builder| {
                    builder.add_col("item_id", |mut c| c.not_null().uuid().clone());
                    builder.add_col("collection_name", |mut c| c.not_null().string().clone());
                    builder.add_col("field_name", |mut c| c.not_null().string().clone());
                    builder.add_col("file_id", |mut c| c.not_null().uuid().clone());
                    builder.add_col("ordinal_position", |mut c| {
                        c.not_null().integer().default(0).clone()
                    });
                    builder.primary_key(
                        Index::create()
                            .col(Alias::new("item_id"))
                            .col(Alias::new("field_name"))
                            .col(Alias::new("file_id"))
                            .primary(),
                    );
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        // Foreign keys (self/child + global users), matching v1's constraints.
        let schema = quote(&self.app_context.schema_name());
        for sql in [
            format!(
                r#"ALTER TABLE {schema}.alcedocore_file_folders
                   ADD CONSTRAINT fk_file_folders_parent
                   FOREIGN KEY (parent_id) REFERENCES {schema}.alcedocore_file_folders(id) ON DELETE CASCADE"#
            ),
            format!(
                r#"ALTER TABLE {schema}.alcedocore_file_folders
                   ADD CONSTRAINT fk_file_folders_created_by
                   FOREIGN KEY (created_by) REFERENCES {}.alcedocore_users(id) ON DELETE SET NULL"#,
                quote(&users.schema_name())
            ),
            format!(
                r#"ALTER TABLE {schema}.alcedocore_file_metadata
                   ADD CONSTRAINT fk_file_metadata_folder
                   FOREIGN KEY (folder_id) REFERENCES {schema}.alcedocore_file_folders(id) ON DELETE SET NULL"#
            ),
            format!(
                r#"ALTER TABLE {schema}.alcedocore_file_metadata
                   ADD CONSTRAINT fk_file_metadata_uploaded_by
                   FOREIGN KEY (uploaded_by) REFERENCES {}.alcedocore_users(id) ON DELETE SET NULL"#,
                quote(&users.schema_name())
            ),
            format!(
                r#"ALTER TABLE {schema}.alcedocore_item_files
                   ADD CONSTRAINT fk_item_files_file
                   FOREIGN KEY (file_id) REFERENCES {schema}.alcedocore_file_metadata(id) ON DELETE CASCADE"#
            ),
        ] {
            sqlx::query(&sql).execute(&mut *connection).await?;
        }

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        for table in [
            "alcedocore_item_files",
            "alcedocore_file_metadata",
            "alcedocore_file_folders",
        ] {
            sqlx::query(&format!(
                r#"DROP TABLE IF EXISTS {schema}."{table}" CASCADE;"#
            ))
            .execute(&mut *connection)
            .await?;
        }
        Ok(())
    }
}

pub(crate) struct M0016Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0016Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0016_files"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m0015_app_settings::M0015Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0016Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
