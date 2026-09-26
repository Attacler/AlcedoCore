use sqlx::{Pool, Postgres};
use sqlx_migrator::Info;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::{Migrate, Migrator, Plan, vec_box};

use crate::migrations::generate_app_state_for_migrations;
use crate::services::context::{AppContext, RequestSource};
use crate::services::items::query::{LogicOp, Query};
use crate::services::items::service::ItemsService;
use crate::services::collections::schema::SchemaService;

pub(crate) mod m00001_init;
pub(crate) mod m00002_views;
pub(crate) mod m00003_fieldoptions;
pub(crate) mod m00004_roles;
pub(crate) mod m00005_collections_ui;
pub(crate) mod m00006_section_related_app;
pub(crate) mod m00007_drop_dead_o2m_columns;
pub(crate) mod m00008_policies_timestamps;
pub(crate) mod m00009_fix_policy_permissions_fk;
pub(crate) mod m00010_user_roles_fk_cascade;

pub(crate) fn migrations(app_context: AppContext) -> Vec<Box<dyn Migration<Postgres>>> {
    vec_box![
        m00001_init::M0001Migration {
            app_context: app_context.clone()
        },
        m00002_views::M0002Migration {
            app_context: app_context.clone()
        },
        m00003_fieldoptions::M0003Migration {
            app_context: app_context.clone()
        },
        m00004_roles::M0004Migration {
            app_context: app_context.clone()
        },
        m00005_collections_ui::M0005Migration {
            app_context: app_context.clone()
        },
        m00006_section_related_app::M0006Migration {
            app_context: app_context.clone()
        },
        m00007_drop_dead_o2m_columns::M0007Migration {
            app_context: app_context.clone()
        },
        m00008_policies_timestamps::M0008Migration {
            app_context: app_context.clone()
        },
        m00009_fix_policy_permissions_fk::M0009Migration {
            app_context: app_context.clone()
        },
        m00010_user_roles_fk_cascade::M0010Migration {
            app_context: app_context.clone()
        }
    ]
}

pub async fn run_app_migrations(database_pool: &Pool<Postgres>) {
    let app_state = generate_app_state_for_migrations().await;

    let app_context = AppContext::system(RequestSource::Migration);
    let collection = "alcedo_apps_versions".to_string();
    let table_service = SchemaService::new(&app_state, &app_context);
    table_service.refresh_schema().await;

    let service = ItemsService::new(&app_state, &app_context, &collection);

    let apps = service
        .read_items_by_query(Query {
            fields: vec![
                "*".to_string(),
                "app_id.*".to_string(),
                "version_id.*".to_string(),
            ],
            filter: LogicOp {
                ..Default::default()
            },
            limit: 0,
            ..Default::default()
        })
        .await
        .expect("Could not query alcedo apps");

    for app in apps {
        let app_context = AppContext {
            app_name: app
                .get("app_id")
                .unwrap()
                .get("api_name")
                .unwrap()
                .as_str()
                .unwrap()
                .to_string(),
            version: app
                .get("version_id")
                .unwrap()
                .get("version_name")
                .unwrap()
                .as_str()
                .unwrap()
                .to_string(),
            request_source: RequestSource::Migration,
        };

        sqlx::query(&format!(
            "CREATE SCHEMA IF NOT EXISTS {}",
            app_context.schema_name()
        ))
        .execute(database_pool)
        .await
        .unwrap();

        let mut migrator = Migrator::default()
            .set_schema(app_context.schema_name())
            .unwrap();

        migrator.add_migrations(migrations(app_context)).unwrap();
        let mut conn = database_pool.acquire().await.unwrap();

        migrator.run(&mut *conn, &Plan::apply_all()).await.unwrap();
    }
}
