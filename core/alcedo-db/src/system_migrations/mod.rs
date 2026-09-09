use alcedo_common::context::{AppContext, RequestSource};
use sqlx::{Pool, Postgres};
use sqlx_migrator::migration::Migration;
use sqlx_migrator::Info;
use sqlx_migrator::{vec_box, Migrate, Migrator, Plan};

pub(crate) mod m00001_init;

pub(crate) fn migrations(app_context: AppContext) -> Vec<Box<dyn Migration<Postgres>>> {
    vec_box![m00001_init::M0001Migration { app_context },]
}

pub async fn run_system_migrations(
    database_pool: &Pool<Postgres>,
) -> Result<(), sqlx_migrator::error::Error> {
    use sqlx_migrator::error::Error;
    let mut migrator = Migrator::default().set_schema("alcedo")?;
    let app_context = AppContext {
        app_name: "alcedo".to_string(),
        version: "".to_string(),
        request_source: RequestSource::Migration,
    };

    sqlx::query(&format!(
        "CREATE SCHEMA IF NOT EXISTS {}",
        app_context.schema_name()
    ))
    .execute(database_pool)
    .await
    .map_err(|e| Error::Box(Box::new(e)))?;

    migrator.add_migrations(migrations(app_context))?;
    let mut conn = database_pool
        .acquire()
        .await
        .map_err(|e| Error::Box(Box::new(e)))?;

    migrator.run(&mut *conn, &Plan::apply_all()).await?;
    Ok(())
}
