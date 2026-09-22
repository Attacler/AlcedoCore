use std::sync::Arc;

use sqlx::{Pool, Postgres};
use sqlx_migrator::Info;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::{Migrate, Migrator, Plan, vec_box};

use crate::services::context::{AppContext, RequestSource};

pub(crate) mod m00001_init;
pub(crate) mod m00002_users;
pub(crate) mod m00003_settings;
pub(crate) mod m00004_sessions;
pub(crate) mod m00005_developer_keys;

pub(crate) fn migrations(app_context: AppContext) -> Vec<Box<dyn Migration<Postgres>>> {
    vec_box![
        m00001_init::M0001Migration {
            app_context: app_context.clone()
        },
        m00002_users::M0002Migration {
            app_context: app_context.clone()
        },
        m00003_settings::M0003Migration {
            app_context: app_context.clone()
        },
        m00004_sessions::M0004Migration {
            app_context: app_context.clone()
        },
        m00005_developer_keys::M0005Migration {
            app_context: app_context.clone()
        },
    ]
}

pub async fn run_system_migrations(database_pool: &Pool<Postgres>) {
    let mut migrator = Migrator::default().set_schema("alcedo").unwrap();
    let app_context = AppContext::system(RequestSource::Migration);

    sqlx::query(&format!(
        "CREATE SCHEMA IF NOT EXISTS {}",
        app_context.schema_name()
    ))
    .execute(database_pool)
    .await
    .unwrap();

    migrator.add_migrations(migrations(app_context)).unwrap();
    let mut conn = database_pool.acquire().await.unwrap();

    migrator.run(&mut *conn, &Plan::apply_all()).await.unwrap();
}
