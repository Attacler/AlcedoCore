    use super::*;
    use crate::{
        item_map,
        migrations::app_migrations::{self, m00011_seed_app_roles::seed_roles},
        services::{
            collections::{
                CreateCollectionRequest, FieldDefinition, UpdateCollectionRequest,
                create_collection, schema::drop_schema, update_collection,
            },
            items::{query::Query, service::ItemsService},
        },
        utils::test_utils::get_app_state,
    };
    use sqlx_migrator::{Info, Migrate};
    use uuid::Uuid;

    use crate::AppState;

    /// Serializes the deploy tests: they create/drop app schemas in the
    /// shared test DB, which must not race each other.
    static DEPLOY_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn test_ctx(app: &str, version: &str) -> AppContext {
        AppContext {
            app_name: app.to_string(),
            version: version.to_string(),
            request_source: RequestSource::SystemTest,
            identity: None,
            request_id: None,
        }
    }

    fn text_field(name: &str) -> FieldDefinition {
        FieldDefinition {
            name: name.to_string(),
            field_type: "string".to_string(),
            ..Default::default()
        }
    }

    async fn scalar_i64(state: &AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&*state.database_pool)
            .await
            .unwrap()
    }

    /// Runs the app migrations for a single `{app}010{version}` schema only.
    /// Unlike `run_app_migrations` (which re-runs every linked app), this does
    /// not touch other tests' schemas when tests run in parallel.
    async fn migrate_schema(pool: &PgPool, app: &str, version: &str) {
        let ctx = AppContext {
            app_name: app.to_string(),
            version: slugify(version),
            request_source: RequestSource::Migration,
            identity: None,
            request_id: None,
        };
        let schema = ctx.schema_name();
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(&format!("REVOKE ALL ON SCHEMA {schema} FROM PUBLIC"))
            .execute(pool)
            .await
            .unwrap();
        let mut migrator = sqlx_migrator::Migrator::default()
            .set_schema(schema)
            .unwrap();
        migrator.add_migrations(app_migrations::migrations(ctx)).unwrap();
        let mut conn = pool.acquire().await.unwrap();
        migrator
            .run(&mut *conn, &sqlx_migrator::Plan::apply_all())
            .await
            .unwrap();
    }

    /// Creates (idempotently) a dedicated app linked to `production` with a
    /// `notes` collection, so deploy tests never touch the shared fixture apps.
    async fn ensure_app(state: &AppState, app: &str) -> i32 {
        let pool = &*state.database_pool;
        let existing: Option<(i32,)> =
            sqlx::query_as("SELECT id FROM alcedocore.alcedocore_apps WHERE api_name = $1")
                .bind(app)
                .fetch_optional(pool)
                .await
                .unwrap();
        let app_id = match existing {
            Some((id,)) => id,
            None => {
                let system = AppContext::system(RequestSource::SystemTest);
                let collection = "alcedocore_apps".to_string();
                let mut svc = ItemsService::new(state, &system, &collection);
                let ids = svc
                    .create_many(
                        vec![item_map! { "name" => app, "api_name" => app }],
                        &mut None,
                    )
                    .await
                    .unwrap();
                ids[0].parse().unwrap()
            }
        };
        let versions = VersionsService::new(state);
        let prod_id = versions
            .get_version_id_by_name(PRODUCTION_VERSION)
            .await
            .unwrap()
            .unwrap();
        let linked: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_apps_versions av \
             JOIN alcedocore.alcedocore_versions v ON v.id = av.version_id \
             WHERE av.app_id = $1 AND v.version_name = 'production'",
        )
        .bind(app_id)
        .fetch_one(pool)
        .await
        .unwrap();
        if linked.0 == 0 {
            AppsService::new(state)
                .link_apps_to_version(&[(app_id, prod_id)])
                .await
                .unwrap();
        }
        migrate_schema(pool, app, PRODUCTION_VERSION).await;
        SchemaService::refresh_schema_and_meta(state).await.unwrap();

        let schema = format!("{}010production", app);
        let has: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM \"{schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        if has.0 == 0 {
            create_collection(
                state,
                &test_ctx(app, PRODUCTION_VERSION),
                CreateCollectionRequest {
                    name: "notes".to_string(),
                    display_name: None,
                    fields: vec![text_field("title")],
                },
            )
            .await
            .unwrap();
            SchemaService::refresh_schema_and_meta(state).await.unwrap();
        }
        app_id
    }

    async fn insert_note(state: &AppState, app: &str, version: &str, title: &str) -> String {
        let ctx = test_ctx(app, version);
        let collection = "notes".to_string();
        let mut svc = ItemsService::new(state, &ctx, &collection);
        let ids = svc
            .create_many(vec![item_map! { "title" => title }], &mut None)
            .await
            .unwrap();
        ids[0].clone()
    }

    /// Creates a fresh version linked only to `keep_app_id`, with just that
    /// app's production schema cloned into it — much faster than
    /// `clone_production_apps`, which clones every production app.
    async fn create_isolated_version(state: &AppState, keep_app_id: i32) -> (i32, String) {
        let versions = VersionsService::new(state);
        let vname = format!("dep{}", &Uuid::new_v4().simple().to_string()[..8]);
        let row = versions.create(&vname).await.unwrap();
        let vid = row.get("id").and_then(Value::as_i64).unwrap() as i32;
        AppsService::new(state)
            .link_apps_to_version(&[(keep_app_id, vid)])
            .await
            .unwrap();
        let pool = &*state.database_pool;
        let (api_name,): (String,) =
            sqlx::query_as("SELECT api_name FROM alcedocore.alcedocore_apps WHERE id = $1")
                .bind(keep_app_id)
                .fetch_one(pool)
                .await
                .unwrap();
        let source = format!("{}010production", api_name);
        let target = format!("{}010{}", api_name, slugify(&vname));
        let copy_tables = copy_set_tables(pool, &source).await.unwrap();
        clone_schema_batched((*pool).clone(), source.clone(), target.clone(), copy_tables)
            .await
            .unwrap();
        // Same order as clone_production_apps: refresh the schema cache first
        // (seed_roles resolves alcedocore_roles through it), then re-seed roles,
        // then recreate the FKs.
        SchemaService::refresh_schema_and_meta(state).await.unwrap();
        let ctx = AppContext {
            app_name: api_name,
            version: slugify(&vname),
            request_source: RequestSource::Migration,
            identity: None,
            request_id: None,
        };
        seed_roles(state, &ctx).await.unwrap();
        recreate_schema_foreign_keys(pool, &source, &target).await.unwrap();
        (vid, vname)
    }

    async fn cleanup_version(state: &AppState, vid: i32, vname: &str) {
        let versions = VersionsService::new(state);
        let app_names = versions.app_api_names_for_version(vid).await.unwrap_or_default();
        let _ = versions.delete_version_links(vid).await;
        let _ = versions.delete(vid).await;
        for app in app_names {
            let _ = drop_schema(state, &format!("{}010{}", app, slugify(vname))).await;
        }
        let _ = SchemaService::refresh_schema_and_meta(state).await;
    }

    async fn cleanup_app(state: &AppState, app: &str) {
        let pool = &*state.database_pool;
        let _ = sqlx::query(
            "DELETE FROM alcedocore.alcedocore_apps_versions WHERE app_id IN \
             (SELECT id FROM alcedocore.alcedocore_apps WHERE api_name = $1)",
        )
        .bind(app)
        .execute(pool)
        .await;
        let _ = sqlx::query("DELETE FROM alcedocore.alcedocore_apps WHERE api_name = $1")
            .bind(app)
            .execute(pool)
            .await;
        let _ = drop_schema(state, &format!("{}010production", app)).await;
        let _ = SchemaService::refresh_schema_and_meta(state).await;
    }

    fn deploy_req(dry_run: bool, on_conflict: OnConflict) -> DeployVersionRequest {
        DeployVersionRequest {
            dry_run,
            on_conflict,
            edit_ids: None,
        }
    }

    fn deploy_req_ids(edit_ids: Vec<Value>, on_conflict: OnConflict) -> DeployVersionRequest {
        DeployVersionRequest {
            dry_run: false,
            on_conflict,
            edit_ids: Some(edit_ids),
        }
    }

    /// Inserts a system-log row into a schema and returns its id (the test app
    /// state's log channel is not drained, so log rows are inserted directly).
    async fn insert_sys_log(state: &AppState, schema: &str, action: &str, target: &str, metadata: Value) -> i64 {
        sqlx::query_scalar(&format!(
            "INSERT INTO {}.alcedocore_system_logs (action, target, metadata) \
             VALUES ($1, $2, $3) RETURNING id",
            qident(schema)
        ))
        .bind(action)
        .bind(target)
        .bind(metadata)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn clone_copy_set_excludes_permissions_and_logs() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtclone";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        // Production-only permission and log rows that must NOT be copied.
        sqlx::query(&format!(
            "INSERT INTO \"{prod_schema}\".alcedocore_roles (id, name, description, is_system) \
             VALUES ($1, 'clone-test-role', '', false)"
        ))
        .bind(Uuid::new_v4())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{prod_schema}\".alcedocore_system_logs (action, target, metadata) \
             VALUES ('setting_changed', 'alcedocore_app_settings', '{{}}')"
        ))
        .execute(pool)
        .await
        .unwrap();
        let _note = insert_note(&state, app, PRODUCTION_VERSION, "prod-note").await;

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));

        // Collections/fields ARE copied (structure + data).
        let prod_collections = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collections")).await;
        let ver_collections = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_collections")).await;
        assert!(prod_collections > 0);
        assert_eq!(ver_collections, prod_collections);
        // Copied collection rows are re-pointed at the target schema, so the
        // meta cache can resolve them there.
        let wrong_app_version: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_collections \
             WHERE app_name <> $1 OR app_version <> $2"
        ))
        .bind(app)
        .bind(slugify(&vname))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(wrong_app_version, 0, "cloned collection rows must carry the target app/version");
        let prod_fields = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_fields")).await;
        let ver_fields = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_fields")).await;
        assert_eq!(ver_fields, prod_fields);

        // Permissions are NOT copied — only the re-seeded system roles exist.
        let ver_roles = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_roles")).await;
        assert_eq!(ver_roles, 2, "expected only the seeded admin/public roles");
        let custom_role = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_roles WHERE name = 'clone-test-role'")).await;
        assert_eq!(custom_role, 0);
        let ver_policies = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_policies")).await;
        assert_eq!(ver_policies, 0);
        // Role-binding tables (layout/menu roles) and plugin→policy bindings
        // are permission data too (they FK-reference the excluded roles/policies).
        for t in [
            "alcedocore_collection_layout_roles",
            "alcedocore_menu_roles",
            "alcedocore_plugin_policies",
        ] {
            assert!(
                !copy_set_tables(pool, &prod_schema).await.unwrap().iter().any(|x| x == t),
                "{t} must be excluded from the copy set"
            );
        }

        // Logs are NOT copied.
        let ver_logs = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_system_logs")).await;
        assert_eq!(ver_logs, 0);
        let ver_coll_logs = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_collection_logs")).await;
        assert_eq!(ver_coll_logs, 0);

        // User tables are structure-only: the table exists but holds no rows.
        let notes_exists: (bool,) = sqlx::query_as("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{}.notes", ver_schema))
            .fetch_one(pool)
            .await
            .unwrap();
        assert!(notes_exists.0);
        let ver_notes = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".notes")).await;
        assert_eq!(ver_notes, 0);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn deploy_happy_path_copies_metadata_and_leaves_permissions_logs_plugins() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dthappy";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        // A production permission row that must survive the deploy untouched.
        sqlx::query(&format!(
            "INSERT INTO \"{prod_schema}\".alcedocore_roles (id, name, description, is_system) \
             VALUES ($1, 'happy-role', '', false)"
        ))
        .bind(Uuid::new_v4())
        .execute(pool)
        .await
        .unwrap();
        let prod_logs_before = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_system_logs")).await;
        let prod_coll_logs_before = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collection_logs")).await;

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));

        // A metadata change in the version: rename the notes collection.
        let ctx = test_ctx(app, &vname);
        let collection = "alcedocore_collections".to_string();
        let mut svc = ItemsService::new(&state, &ctx, &collection);
        let mut q = Query::eq("name", json!("notes"));
        svc.update_items_by_query(&mut q, item_map! { "name" => "Deployed Notes" }, &mut None)
            .await
            .unwrap();
        // The edit is enumerated from the version's logs.
        let notes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{ver_schema}\".alcedocore_system_logs (action, target, metadata) \
             VALUES ('collection_updated', 'alcedocore_collections', $1)"
        ))
        .bind(json!({ "id": notes_id, "name": "notes" }))
        .execute(pool)
        .await
        .unwrap();

        // A plugin installed on the version's app-version + a route mapping.
        let version_av_id: i32 = sqlx::query_scalar(
            "SELECT id FROM alcedocore.alcedocore_apps_versions WHERE app_id = $1 AND version_id = $2",
        )
        .bind(app_id)
        .bind(vid)
        .fetch_one(pool)
        .await
        .unwrap();
        let slug = format!("deploy-test-{}", &vname[3..]);
        let prefix = format!("/dt{}", &vname[3..]);
        let plugin_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins (slug, registry_id, plugin_type, image) \
             VALUES ($1, 1, 'dynamic', 'localhost:5000/deploy-test:1.0.0') RETURNING id",
        )
        .bind(&slug)
        .fetch_one(pool)
        .await
        .unwrap();
        let old_install_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins_installs \
             (plugin_id, app_version_id, plugin_version, enabled) \
             VALUES ($1, $2, '1.0.0', true) RETURNING id",
        )
        .bind(plugin_id)
        .bind(version_av_id)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO alcedocore.alcedocore_route_mappings (domain, path_prefix, install_id) \
             VALUES ('deploy-test.example', $1, $2)",
        )
        .bind(&prefix)
        .bind(old_install_id)
        .execute(pool)
        .await
        .unwrap();

        let start = chrono::Utc::now();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));
        assert_eq!(resp.data["edits"].as_array().unwrap().len(), 1);
        assert_eq!(resp.data["warning"].as_str().unwrap(), NO_BACKUP_WARNING);
        assert!(resp.data.get("backup_version").is_none(), "no backup version is created anymore");

        // Metadata was copied to production.
        let dn: (Option<String>,) = sqlx::query_as(&format!(
            "SELECT name FROM \"{prod_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(dn.0.as_deref(), Some("Deployed Notes"));

        // Deployed collection rows are re-pointed at production, so the meta
        // cache resolves them in the production schema.
        let wrong_app_version: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collections \
             WHERE app_name <> $1 OR app_version <> 'production'"
        ))
        .bind(app)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(wrong_app_version, 0, "deployed collection rows must carry production app/version");

        // Permissions and app-schema logs in production are untouched.
        let happy_role = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_roles WHERE name = 'happy-role'")).await;
        assert_eq!(happy_role, 1);
        let prod_logs_after = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_system_logs")).await;
        assert_eq!(prod_logs_after, prod_logs_before);
        let prod_coll_logs_after = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collection_logs")).await;
        assert_eq!(prod_coll_logs_after, prod_coll_logs_before);

        // version_deployed was logged to the global system log.
        let logged: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_system_logs \
             WHERE action = 'version_deployed' AND target = $1",
        )
        .bind(&vname)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(logged.0, 1);

        // No backup version is created before writing.
        let backups: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_versions WHERE version_name LIKE 'backup_before_deploy_%'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(backups.0, 0);

        // The plugin install was copied with app_version_id remapped to
        // production's app-version id (new install id), and the route mapping
        // was copied with install_id remapped to the new install id.
        let prod_av_id: i32 = sqlx::query_scalar(
            "SELECT av.id FROM alcedocore.alcedocore_apps_versions av \
             JOIN alcedocore.alcedocore_versions v ON v.id = av.version_id \
             WHERE av.app_id = $1 AND v.version_name = 'production'",
        )
        .bind(app_id)
        .fetch_one(pool)
        .await
        .unwrap();
        let new_install: Option<(i32, i32)> = sqlx::query_as(
            "SELECT id, app_version_id FROM alcedocore.alcedocore_plugins_installs \
             WHERE plugin_id = $1 AND app_version_id = $2",
        )
        .bind(plugin_id)
        .bind(prod_av_id)
        .fetch_optional(pool)
        .await
        .unwrap();
        let (new_install_id, _) = new_install.expect("install should exist in production");
        assert_ne!(new_install_id, old_install_id);
        // The deploy provisioned the runtime for the new production install:
        // a deployment_id is recorded (Swarm service on docker, id on mock).
        let (dep_id, pv): (Option<String>, String) = sqlx::query_as(
            "SELECT deployment_id, plugin_version FROM alcedocore.alcedocore_plugins_installs WHERE id = $1",
        )
        .bind(new_install_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(
            dep_id.is_some(),
            "deploy must provision the plugin runtime (deployment_id) for the new production install"
        );
        assert_eq!(pv, "1.0.0");
        let rm_install: (i32,) = sqlx::query_as(
            "SELECT install_id FROM alcedocore.alcedocore_route_mappings WHERE path_prefix = $1",
        )
        .bind(&prefix)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(rm_install.0, new_install_id);

        // Cleanup.
        let _ = sqlx::query("DELETE FROM alcedocore.alcedocore_route_mappings WHERE path_prefix = $1").bind(&prefix).execute(pool).await;
        let _ = sqlx::query("DELETE FROM alcedocore.alcedocore_plugins WHERE slug = $1").bind(&slug).execute(pool).await;
        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// A plugin installed in a version must get its runtime in production on
    /// deploy (deployment_id recorded), and a plugin VERSION bump in the
    /// version must re-provision the existing production install on re-deploy
    /// (same install id, new plugin_version) instead of duplicating it.
    #[tokio::test]
    async fn deploy_provisions_plugin_runtime_and_version_bump_reprovisions() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtplugv";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let version_av_id: i32 = sqlx::query_scalar(
            "SELECT id FROM alcedocore.alcedocore_apps_versions WHERE app_id = $1 AND version_id = $2",
        )
        .bind(app_id)
        .bind(vid)
        .fetch_one(pool)
        .await
        .unwrap();
        let slug = format!("deploy-plugv-{}", &vname[3..]);
        let plugin_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins (slug, registry_id, plugin_type, image) \
             VALUES ($1, 1, 'dynamic', 'localhost:5000/deploy-plugv:1.0.0') RETURNING id",
        )
        .bind(&slug)
        .fetch_one(pool)
        .await
        .unwrap();
        let old_install_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins_installs \
             (plugin_id, app_version_id, plugin_version, enabled) \
             VALUES ($1, $2, '1.0.0', true) RETURNING id",
        )
        .bind(plugin_id)
        .bind(version_av_id)
        .fetch_one(pool)
        .await
        .unwrap();

        let prod_av_id: i32 = sqlx::query_scalar(
            "SELECT av.id FROM alcedocore.alcedocore_apps_versions av \
             JOIN alcedocore.alcedocore_versions v ON v.id = av.version_id \
             WHERE av.app_id = $1 AND v.version_name = 'production'",
        )
        .bind(app_id)
        .fetch_one(pool)
        .await
        .unwrap();

        // First deploy (wholesale): install copied to production + runtime.
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "first deploy should succeed: {}", resp.data);
        let (inst1, dep1, pv1): (i32, Option<String>, String) = sqlx::query_as(
            "SELECT id, deployment_id, plugin_version FROM alcedocore.alcedocore_plugins_installs \
             WHERE plugin_id = $1 AND app_version_id = $2",
        )
        .bind(plugin_id)
        .bind(prod_av_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_ne!(inst1, old_install_id);
        assert!(dep1.is_some(), "new production install must get a deployment_id (runtime provisioned)");
        assert_eq!(pv1, "1.0.0");

        // Bump the plugin version in the version's install, then re-deploy.
        sqlx::query(
            "UPDATE alcedocore.alcedocore_plugins_installs SET plugin_version = '2.0.0' WHERE id = $1",
        )
        .bind(old_install_id)
        .execute(pool)
        .await
        .unwrap();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "re-deploy should succeed: {}", resp.data);
        let installs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_plugins_installs \
             WHERE plugin_id = $1 AND app_version_id = $2",
        )
        .bind(plugin_id)
        .bind(prod_av_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(installs, 1, "re-deploy must not duplicate the install");
        let (dep2, pv2): (Option<String>, String) = sqlx::query_as(
            "SELECT deployment_id, plugin_version FROM alcedocore.alcedocore_plugins_installs WHERE id = $1",
        )
        .bind(inst1)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(pv2, "2.0.0", "version bump must update the production install");
        assert!(dep2.is_some(), "re-provisioned install keeps a deployment_id");

        let _ = sqlx::query("DELETE FROM alcedocore.alcedocore_plugins_installs WHERE plugin_id = $1")
            .bind(plugin_id)
            .execute(pool)
            .await;
        let _ = sqlx::query("DELETE FROM alcedocore.alcedocore_plugins WHERE slug = $1")
            .bind(&slug)
            .execute(pool)
            .await;
        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn deploy_then_revert_undoes_applied_changes_without_backup() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtrevert";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));

        // A change in the version: a new collection.
        create_collection(
            &state,
            &test_ctx(app, &vname),
            CreateCollectionRequest {
                name: "revnotes".to_string(),
                display_name: None,
                fields: vec![text_field("title")],
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        let revnotes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'revnotes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        insert_sys_log(
            &state,
            &ver_schema,
            "collection_created",
            "alcedocore_collections",
            json!({ "id": revnotes_id, "name": "revnotes" }),
        )
        .await;

        // Deploy: no backup version is created, the warning is present, and the
        // collection lands in production.
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Overwrite))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["warning"].as_str().unwrap(), NO_BACKUP_WARNING);
        assert!(resp.data.get("backup_version").is_none());
        let has: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = '{prod_schema}' AND table_name = 'revnotes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(has.0, 1, "deployed collection table must exist in production");

        // Revert: the collection_created edit is inverted (metadata row
        // deleted, user table dropped).
        let r = VersionsService::new(&state).revert(vid).await.unwrap();
        assert!(r.ok, "revert should succeed: {}", r.data);
        assert_eq!(r.data["warning"].as_str().unwrap(), NO_BACKUP_WARNING);
        assert!(
            !r.data["reverted"].as_array().unwrap().is_empty(),
            "revert must list the undone edits: {}",
            r.data
        );
        let has: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = '{prod_schema}' AND table_name = 'revnotes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(has.0, 0, "revert must drop the deployed collection table");
        let meta: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collections WHERE \"table\" = 'revnotes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(meta.0, 0, "revert must delete the collection metadata row");

        // The revert is recorded in the markers: a second revert has nothing
        // left to undo.
        let r2 = VersionsService::new(&state).revert(vid).await.unwrap();
        assert!(
            r2.data["reverted"].as_array().unwrap().is_empty(),
            "second revert must be a no-op: {}",
            r2.data
        );

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn deploy_marks_applied_edits_and_hides_them_on_redeploy() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtmark";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));

        // One system-log edit in the version.
        let edit_id = insert_sys_log(
            &state,
            &ver_schema,
            "collection_updated",
            "alcedocore_collections",
            json!({ "id": 1, "name": "notes" }),
        )
        .await;

        let start = chrono::Utc::now();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["edits"].as_array().unwrap().len(), 1);

        // The deploy marker records the applied edit id for the app, and the
        // response carries the applied map too.
        let applied = applied_edits_for_version(pool, &vname).await.unwrap();
        let (ids, apps) = applied.get(app).expect("app entry in applied map");
        assert!(ids.contains(&edit_id));
        assert!(apps.is_empty());
        assert!(resp.data["applied"][app]["system_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_i64() == Some(edit_id)));

        // A second deploy (dry run) no longer lists the edit as pending.
        let resp2 = VersionsService::new(&state)
            .deploy(vid, &deploy_req(true, OnConflict::Fail))
            .await
            .unwrap();
        assert_eq!(resp2.data["edits"].as_array().unwrap().len(), 0);

        // Selecting the already-deployed id applies nothing.
        let resp3 = VersionsService::new(&state)
            .deploy(vid, &deploy_req_ids(vec![json!(edit_id)], OnConflict::Fail))
            .await
            .unwrap();
        assert_eq!(resp3.data["edits"].as_array().unwrap().len(), 0);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn deploy_conflict_fail_returns_structured_conflicts_and_writes_nothing() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtconflict";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));

        let notes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{prod_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();

        // Version-side edit: the notes collection was updated in the version.
        sqlx::query(&format!(
            "INSERT INTO \"{ver_schema}\".alcedocore_system_logs (action, target, metadata) \
             VALUES ('collection_updated', 'alcedocore_collections', $1)"
        ))
        .bind(json!({ "id": notes_id, "name": "notes" }))
        .execute(pool)
        .await
        .unwrap();
        // Version-side edit: a new collection that does not exist in production.
        sqlx::query(&format!(
            "INSERT INTO \"{ver_schema}\".alcedocore_system_logs (action, target, metadata) \
             VALUES ('collection_created', 'alcedocore_collections', $1)"
        ))
        .bind(json!({ "id": 999999, "name": "invoices" }))
        .execute(pool)
        .await
        .unwrap();
        // Production-side change after the clone timestamp: same target+item.
        sqlx::query(&format!(
            "INSERT INTO \"{prod_schema}\".alcedocore_system_logs (action, target, metadata) \
             VALUES ('collection_updated', 'alcedocore_collections', $1)"
        ))
        .bind(json!({ "id": notes_id, "name": "notes" }))
        .execute(pool)
        .await
        .unwrap();

        let start = chrono::Utc::now();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(!resp.ok, "conflicts with on_conflict=fail must be refused");
        assert_eq!(resp.data["deployed"], json!(false));
        let conflicts = resp.data["conflicts"].as_array().unwrap();
        assert!(
            conflicts.iter().any(|c| c["type"] == "changed_on_both_sides"
                && c["app"] == app
                && c["prod_action"] == "collection_updated"
                && c["version_action"] == "collection_updated"),
            "expected a changed_on_both_sides conflict: {}",
            resp.data
        );
        assert!(
            conflicts.iter().any(|c| c["type"] == "relation_missing_in_prod"
                && c["app"] == app
                && c["collection"] == "invoices"),
            "expected a relation_missing_in_prod conflict: {}",
            resp.data
        );

        // Nothing was written: no deploy log row; the response warns about no backup.
        assert_eq!(resp.data["warning"].as_str().unwrap(), NO_BACKUP_WARNING);
        let logged: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_system_logs \
             WHERE action = 'version_deployed' AND target = $1",
        )
        .bind(&vname)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(logged.0, 0);

        // The controller renders the refusal as HTTP 409 + JSend fail.
        let prod_id = VersionsService::new(&state)
            .get_version_id_by_name(PRODUCTION_VERSION)
            .await
            .unwrap()
            .unwrap();
        let http_resp = crate::controllers::versions::deploy_version(
            axum::extract::State(state.clone()),
            crate::middelware::auth::AuthLevel::DeveloperKey { version_id: prod_id },
            axum::extract::Path(vid),
            axum::Json(deploy_req(false, OnConflict::Fail)),
        )
        .await
        .unwrap();
        assert_eq!(http_resp.status(), axum::http::StatusCode::CONFLICT);
        let body = axum::body::to_bytes(http_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["status"], json!("fail"));
        assert_eq!(body["data"]["deployed"], json!(false));
        assert!(body["data"]["conflicts"].as_array().unwrap().len() >= 2);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn deploy_dry_run_returns_edits_and_writes_nothing() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtdry";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));

        let ctx = test_ctx(app, &vname);
        let collection = "alcedocore_collections".to_string();
        let mut svc = ItemsService::new(&state, &ctx, &collection);
        let mut q = Query::eq("name", json!("notes"));
        svc.update_items_by_query(&mut q, item_map! { "name" => "Dry Notes" }, &mut None)
            .await
            .unwrap();
        let notes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{ver_schema}\".alcedocore_system_logs (action, target, metadata) \
             VALUES ('collection_updated', 'alcedocore_collections', $1)"
        ))
        .bind(json!({ "id": notes_id, "name": "notes" }))
        .execute(pool)
        .await
        .unwrap();

        let start = chrono::Utc::now();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(true, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data["dry_run"], json!(true));
        assert_eq!(resp.data["deployed"], json!(false));
        assert_eq!(resp.data["edits"].as_array().unwrap().len(), 1);

        // Production was not modified (display name still the original).
        let dn: (Option<String>,) = sqlx::query_as(&format!(
            "SELECT name FROM \"{prod_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(dn.0.as_deref(), Some("notes"));
        assert_eq!(resp.data["warning"].as_str().unwrap(), NO_BACKUP_WARNING);
        let logged: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_system_logs \
             WHERE action = 'version_deployed' AND target = $1",
        )
        .bind(&vname)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(logged.0, 0);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn deploy_new_app_links_and_clones_it_into_production() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let holder = "dthold";
        let holder_id = ensure_app(&state, holder).await;
        let new_app = "dtnew";

        let (vid, vname) = create_isolated_version(&state, holder_id).await;

        // Create an app that is linked only to the version.
        let pool = &*state.database_pool;
        let system = AppContext::system(RequestSource::SystemTest);
        let collection = "alcedocore_apps".to_string();
        let mut svc = ItemsService::new(&state, &system, &collection);
        let ids = svc
            .create_many(
                vec![item_map! { "name" => new_app, "api_name" => new_app }],
                &mut None,
            )
            .await
            .unwrap();
        let new_app_id: i32 = ids[0].parse().unwrap();
        AppsService::new(&state)
            .link_apps_to_version(&[(new_app_id, vid)])
            .await
            .unwrap();
        migrate_schema(pool, new_app, &vname).await;
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        create_collection(
            &state,
            &test_ctx(new_app, &vname),
            CreateCollectionRequest {
                name: "newcoll".to_string(),
                display_name: None,
                fields: vec![text_field("label")],
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();

        let start = chrono::Utc::now();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));
        assert!(
            resp.data["apps"].as_array().unwrap().iter().any(|a| a["app"] == new_app && a["status"] == "created"),
            "expected the new app in the result: {}",
            resp.data
        );
        assert!(
            resp.data["edits"].as_array().unwrap().iter().any(|e| e["action"] == "app_created" && e["app"] == new_app),
            "expected an app_created edit: {}",
            resp.data
        );

        // The app is now linked to production and its schema was cloned.
        let linked: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_apps_versions av \
             JOIN alcedocore.alcedocore_versions v ON v.id = av.version_id \
             WHERE av.app_id = $1 AND v.version_name = 'production'",
        )
        .bind(new_app_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(linked.0, 1);
        let prod_schema = format!("{}010production", new_app);
        let newcoll: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collections WHERE name = 'newcoll'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(newcoll.0, 1);

        // The new app's production schema got its roles seeded (the migrator
        // table is copied, so m00011 never runs for it).
        let seeded_roles: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_roles WHERE name IN ('admin', 'public')"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(seeded_roles.0, 2, "new app production schema must have admin+public roles");

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, new_app).await;
        cleanup_app(&state, holder).await;
    }

    #[tokio::test]
    async fn deploy_remaps_cross_app_relation_fks_to_production_schemas() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let holder = "dtxhold";
        let other = "dtxother";
        let holder_id = ensure_app(&state, holder).await;
        let (vid, vname) = create_isolated_version(&state, holder_id).await;
        let vslug = slugify(&vname);

        // A second app linked only to the version, owning the referenced collection.
        let pool = &*state.database_pool;
        let system = AppContext::system(RequestSource::SystemTest);
        let collection = "alcedocore_apps".to_string();
        let mut svc = ItemsService::new(&state, &system, &collection);
        let ids = svc
            .create_many(
                vec![item_map! { "name" => other, "api_name" => other }],
                &mut None,
            )
            .await
            .unwrap();
        let other_id: i32 = ids[0].parse().unwrap();
        AppsService::new(&state)
            .link_apps_to_version(&[(other_id, vid)])
            .await
            .unwrap();
        migrate_schema(pool, other, &vname).await;
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        create_collection(
            &state,
            &test_ctx(other, &vname),
            CreateCollectionRequest {
                name: "othercoll".to_string(),
                display_name: None,
                fields: vec![text_field("label")],
            },
        )
        .await
        .unwrap();

        // A collection in the holder app whose relation field points at the
        // OTHER app's collection (cross-app `related_app`).
        create_collection(
            &state,
            &test_ctx(holder, &vname),
            CreateCollectionRequest {
                name: "xorders".to_string(),
                display_name: None,
                fields: vec![
                    text_field("title"),
                    FieldDefinition {
                        name: "other_ref".to_string(),
                        field_type: "relationship".to_string(),
                        relationship_type: Some("many_to_one".to_string()),
                        related_app: Some(other.to_string()),
                        related_collection: Some("othercoll".to_string()),
                        ..Default::default()
                    },
                ],
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();

        // The test app state's log channel is not drained, so record the
        // collection_created edit directly (as the activity-log hook would).
        let holder_ver_schema = format!("{}010{}", holder, vslug);
        insert_sys_log(
            &state,
            &holder_ver_schema,
            "collection_created",
            "alcedocore_collections",
            json!({ "name": "xorders", "id": scalar_i64(&state, &format!(
                "SELECT id::bigint FROM \"{holder_ver_schema}\".alcedocore_collections WHERE name = 'xorders'"
            )).await }),
        )
        .await;

        // Sanity: in the version schema the FK points at the other app's VERSION schema.
        let ver_fk: String = sqlx::query_scalar(&format!(
            "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
             WHERE contype = 'f' AND conrelid = '{}010{}.xorders'::regclass \
               AND pg_get_constraintdef(oid) LIKE '%othercoll%'",
            holder, vslug
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(
            ver_fk.contains(&format!("{other}010{vslug}.")),
            "version FK should reference the version schema: {ver_fk}"
        );

        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Overwrite))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));

        // After deploy, the FK in the holder's PRODUCTION schema must reference
        // the other app's PRODUCTION schema — not the version schema.
        let prod_fk: String = sqlx::query_scalar(&format!(
            "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
             WHERE contype = 'f' AND conrelid = '{}010production.xorders'::regclass \
               AND pg_get_constraintdef(oid) LIKE '%othercoll%'",
            holder
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(
            prod_fk.contains(&format!("{other}010production.othercoll")),
            "prod FK must reference the other app's production schema: {prod_fk}"
        );
        assert!(
            !prod_fk.contains(&format!("010{vslug}.")),
            "prod FK must not reference the version schema: {prod_fk}"
        );

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, other).await;
        cleanup_app(&state, holder).await;
    }

    #[tokio::test]
    async fn deploy_refuses_production_version_and_version_without_apps() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let versions = VersionsService::new(&state);
        let prod_id = versions
            .get_version_id_by_name(PRODUCTION_VERSION)
            .await
            .unwrap()
            .unwrap();

        let err = versions
            .deploy(prod_id, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::InvalidInput(_, _)));

        // A version with no linked apps is refused.
        let vname = format!("dep{}", &Uuid::new_v4().simple().to_string()[..8]);
        let row = versions.create(&vname).await.unwrap();
        let vid = row.get("id").and_then(Value::as_i64).unwrap() as i32;
        sqlx::query("DELETE FROM alcedocore.alcedocore_apps_versions WHERE version_id = $1")
            .bind(vid)
            .execute(&*state.database_pool)
            .await
            .unwrap();
        let err = versions
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::InvalidInput(_, _)));

        let _ = versions.delete(vid).await;
    }

    #[tokio::test]
    async fn deploy_edit_ids_applies_only_the_selected_edits() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtselect";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));

        // Edit A: rename the notes collection in the version.
        let ctx = test_ctx(app, &vname);
        let collection = "alcedocore_collections".to_string();
        let mut svc = ItemsService::new(&state, &ctx, &collection);
        let mut q = Query::eq("name", json!("notes"));
        svc.update_items_by_query(&mut q, item_map! { "name" => "Selected Notes" }, &mut None)
            .await
            .unwrap();
        let notes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let edit_a = insert_sys_log(&state, &ver_schema, "collection_updated", "alcedocore_collections", json!({ "id": notes_id, "name": "notes" })).await;

        // Edit B: a new collection created in the version (not selected).
        create_collection(
            &state,
            &test_ctx(app, &vname),
            CreateCollectionRequest {
                name: "extra".to_string(),
                display_name: None,
                fields: vec![text_field("label")],
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        let extra_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'extra'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let _edit_b = insert_sys_log(&state, &ver_schema, "collection_created", "alcedocore_collections", json!({ "id": extra_id, "name": "extra" })).await;

        // Deploy only edit A. The string entry (an app api_name, as the UI
        // sends for app_created) must not 400 the request.
        let start = chrono::Utc::now();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req_ids(vec![json!(edit_a), json!(app)], OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));
        // Only the selected edit was enumerated.
        assert_eq!(resp.data["edits"].as_array().unwrap().len(), 1);
        assert_eq!(resp.data["edits"][0]["id"], json!(edit_a));

        // Edit A was applied to production …
        let dn: (Option<String>,) = sqlx::query_as(&format!(
            "SELECT name FROM \"{prod_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(dn.0.as_deref(), Some("Selected Notes"));
        // … and edit B was NOT: no extra table, no extra metadata row.
        let extra_table: (bool,) = sqlx::query_as("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("{}.extra", prod_schema))
            .fetch_one(pool)
            .await
            .unwrap();
        assert!(!extra_table.0, "unselected collection_created edit must not create the table");
        let extra_row = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collections WHERE \"table\" = 'extra'")).await;
        assert_eq!(extra_row, 0);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// Adds a `body` field to the version's `notes` collection (runs ADD COLUMN
    /// in the version schema) and records the field edit in the version's
    /// system log. Returns (edit log-row id, version schema).
    async fn add_body_field_edit(state: &AppState, app: &str, vname: &str) -> (i64, String) {
        let pool = &*state.database_pool;
        let ver_schema = format!("{}010{}", app, slugify(vname));
        update_collection(
            state,
            &test_ctx(app, vname),
            "notes",
            UpdateCollectionRequest {
                fields: vec![text_field("title"), text_field("body")],
                removed_fields: vec![],
                display_name: None,
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(state).await.unwrap();
        let field_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_fields WHERE api_name = 'body'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let notes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let edit_id = insert_sys_log(
            state,
            &ver_schema,
            "item_created",
            "alcedocore_fields",
            json!({
                "id": field_id,
                "api_name": "body",
                "collection_id": notes_id,
                "options": { "name": "body", "type": "string" },
            }),
        )
        .await;
        (edit_id, ver_schema)
    }

    async fn column_exists(pool: &PgPool, schema: &str, table: &str, column: &str) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_schema = $1 AND table_name = $2 AND column_name = $3)",
        )
        .bind(schema)
        .bind(table)
        .bind(column)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// Regression test: a field added in a version must deploy its PHYSICAL
    /// column to the production user table, not just the alcedocore_fields
    /// metadata row — otherwise the items API rejects writes with
    /// "Unknown field <name> provided".
    #[tokio::test]
    async fn deploy_wholesale_adds_field_column_to_production_user_table() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtfieldw";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);
        assert!(!column_exists(pool, &prod_schema, "notes", "body").await);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let (_edit_id, ver_schema) = add_body_field_edit(&state, app, &vname).await;
        assert!(column_exists(pool, &ver_schema, "notes", "body").await);

        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));

        // The physical column exists in production now …
        assert!(
            column_exists(pool, &prod_schema, "notes", "body").await,
            "wholesale deploy must add the field's column to the production user table"
        );
        // … the metadata row was copied …
        let prod_field = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_fields WHERE api_name = 'body'"
        ))
        .await;
        assert_eq!(prod_field, 1);
        // … and the items API accepts the new field in production (the exact
        // "Unknown field" failure reported from the admin UI).
        let ctx = test_ctx(app, PRODUCTION_VERSION);
        let collection = "notes".to_string();
        let mut svc = ItemsService::new(&state, &ctx, &collection);
        let ids = svc
            .create_many(vec![item_map! { "title" => "t", "body" => "hello" }], &mut None)
            .await
            .unwrap();
        assert_eq!(ids.len(), 1);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// Same regression for selected mode (`edit_ids` = the field edit only):
    /// the column sync must run for the collections that have field edits.
    #[tokio::test]
    async fn deploy_selected_field_edit_adds_column_to_production_user_table() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtfields";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);
        assert!(!column_exists(pool, &prod_schema, "notes", "body").await);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let (edit_id, ver_schema) = add_body_field_edit(&state, app, &vname).await;
        assert!(column_exists(pool, &ver_schema, "notes", "body").await);

        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req_ids(vec![json!(edit_id)], OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));
        assert_eq!(resp.data["edits"].as_array().unwrap().len(), 1);

        assert!(
            column_exists(pool, &prod_schema, "notes", "body").await,
            "selected deploy must add the field's column to the production user table"
        );
        let prod_field = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_fields WHERE api_name = 'body'"
        ))
        .await;
        assert_eq!(prod_field, 1);
        let ctx = test_ctx(app, PRODUCTION_VERSION);
        let collection = "notes".to_string();
        let mut svc = ItemsService::new(&state, &ctx, &collection);
        let ids = svc
            .create_many(vec![item_map! { "title" => "t", "body" => "hello" }], &mut None)
            .await
            .unwrap();
        assert_eq!(ids.len(), 1);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// A wholesale re-deploy of a version whose edits are all already applied
    /// must still sync missing user-table columns (additive only) — deploys
    /// made before the column sync existed copied field metadata without the
    /// physical column.
    #[tokio::test]
    async fn deploy_redeploy_after_applied_still_syncs_missing_columns() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtfieldr";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let (_edit_id, ver_schema) = add_body_field_edit(&state, app, &vname).await;

        // First wholesale deploy: applies the edit and syncs the column.
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "first deploy should succeed: {}", resp.data);
        assert!(column_exists(pool, &prod_schema, "notes", "body").await);

        // Simulate a pre-fix deploy: metadata row present, physical column gone.
        sqlx::query(&format!(
            "ALTER TABLE \"{prod_schema}\".notes DROP COLUMN body"
        ))
        .execute(pool)
        .await
        .unwrap();
        assert!(!column_exists(pool, &prod_schema, "notes", "body").await);

        // Re-deploy: no pending edits, but the column sync must still run.
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "re-deploy should succeed: {}", resp.data);
        assert!(
            column_exists(pool, &prod_schema, "notes", "body").await,
            "re-deploy must re-add the missing physical column"
        );
        // The version schema is untouched.
        assert!(column_exists(pool, &ver_schema, "notes", "body").await);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// The FK definition on `schema.table` whose constrained column is `col`
    /// (e.g. `FOREIGN KEY (parent) REFERENCES ...`), if any.
    async fn fk_def_for(pool: &PgPool, schema: &str, table: &str, col: &str) -> Option<String> {
        sqlx::query_scalar(
            "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
             WHERE contype = 'f' AND conrelid = $1::regclass \
               AND pg_get_constraintdef(oid) LIKE $2",
        )
        .bind(format!("{}.{}", qident(schema), qident(table)))
        .bind(format!("%({col})%"))
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    /// Whether that FK is validated (`convalidated`).
    async fn fk_validated(pool: &PgPool, schema: &str, table: &str, col: &str) -> Option<bool> {
        sqlx::query_scalar(
            "SELECT convalidated FROM pg_constraint \
             WHERE contype = 'f' AND conrelid = $1::regclass \
               AND pg_get_constraintdef(oid) LIKE $2",
        )
        .bind(format!("{}.{}", qident(schema), qident(table)))
        .bind(format!("%({col})%"))
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    /// Adds a many-to-one relation field (`parent` → notes.id) to the EXISTING
    /// `notes` collection in the version — the real API flow, which creates the
    /// physical FK in the version schema — and records the field edit in the
    /// system log. Returns (edit id, version schema).
    async fn add_relation_field_edit(state: &AppState, app: &str, vname: &str) -> (i64, String) {
        let pool = &*state.database_pool;
        let ver_schema = format!("{}010{}", app, slugify(vname));
        update_collection(
            state,
            &test_ctx(app, vname),
            "notes",
            UpdateCollectionRequest {
                fields: vec![
                    text_field("title"),
                    FieldDefinition {
                        name: "parent".to_string(),
                        field_type: "relationship".to_string(),
                        relationship_type: Some("many_to_one".to_string()),
                        related_collection: Some("notes".to_string()),
                        ..Default::default()
                    },
                ],
                removed_fields: vec![],
                display_name: None,
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(state).await.unwrap();
        // Sanity: the version schema has the physical FK for the relation field.
        assert!(
            fk_def_for(pool, &ver_schema, "notes", "parent").await.is_some(),
            "version schema must have the FK for the relation field"
        );
        let field_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_fields WHERE api_name = 'parent'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let notes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let edit_id = insert_sys_log(
            state,
            &ver_schema,
            "item_created",
            "alcedocore_fields",
            json!({
                "id": field_id,
                "api_name": "parent",
                "collection_id": notes_id,
                "options": { "name": "parent", "type": "relationship" },
            }),
        )
        .await;
        (edit_id, ver_schema)
    }

    /// A relation field added to an EXISTING collection in a version must deploy
    /// its physical FK to the production user table (wholesale mode), remapped
    /// to the production schema — not just the alcedocore_fields metadata row.
    #[tokio::test]
    async fn deploy_wholesale_adds_missing_fk_to_existing_production_table() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtfkw";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);
        assert!(fk_def_for(pool, &prod_schema, "notes", "parent").await.is_none());

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let (_edit_id, _ver_schema) = add_relation_field_edit(&state, app, &vname).await;

        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));

        let prod_fk = fk_def_for(pool, &prod_schema, "notes", "parent")
            .await
            .expect("wholesale deploy must add the relation FK to the production user table");
        assert!(
            prod_fk.contains(&format!("{prod_schema}.notes")),
            "prod FK must reference the production schema: {prod_fk}"
        );
        assert!(
            !prod_fk.contains(&format!("010{}.", slugify(&vname))),
            "prod FK must not reference the version schema: {prod_fk}"
        );
        assert_eq!(
            fk_validated(pool, &prod_schema, "notes", "parent").await,
            Some(true),
            "with no violating rows the FK should be validated"
        );

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// Same as above, but selected mode (`edit_ids` = the field edit only): the
    /// FK sync must run for the collections that have field edits.
    #[tokio::test]
    async fn deploy_selected_field_edit_adds_fk_to_existing_production_table() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtfks";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);
        assert!(fk_def_for(pool, &prod_schema, "notes", "parent").await.is_none());

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let (edit_id, _ver_schema) = add_relation_field_edit(&state, app, &vname).await;

        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req_ids(vec![json!(edit_id)], OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));

        let prod_fk = fk_def_for(pool, &prod_schema, "notes", "parent")
            .await
            .expect("selected deploy must add the relation FK to the production user table");
        assert!(
            prod_fk.contains(&format!("{prod_schema}.notes")),
            "prod FK must reference the production schema: {prod_fk}"
        );

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// When production already holds rows that would violate the new FK, the
    /// deploy must still succeed: the FK is added NOT VALID (savepoint
    /// fallback) instead of aborting the whole transaction.
    #[tokio::test]
    async fn deploy_fk_falls_back_to_not_valid_when_prod_rows_violate() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtfknv";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let (_edit_id, _ver_schema) = add_relation_field_edit(&state, app, &vname).await;

        // Simulate a pre-sync production table: the column exists (as an old
        // deploy would have left it) with an orphan row pointing nowhere.
        sqlx::query(&format!(
            "ALTER TABLE \"{prod_schema}\".notes ADD COLUMN parent uuid"
        ))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO \"{prod_schema}\".notes (title, parent) VALUES ('orphan', gen_random_uuid())"
        ))
        .execute(pool)
        .await
        .unwrap();

        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(
            resp.ok,
            "deploy must succeed despite violating rows (NOT VALID fallback): {}",
            resp.data
        );
        assert_eq!(resp.data["deployed"], json!(true));

        assert!(
            fk_def_for(pool, &prod_schema, "notes", "parent").await.is_some(),
            "the FK must exist after deploy"
        );
        assert_eq!(
            fk_validated(pool, &prod_schema, "notes", "parent").await,
            Some(false),
            "the FK must be NOT VALID when production rows violate it"
        );

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// Cross-app variant on an EXISTING collection: a relation field in the
    /// holder app pointing at the other app's collection must deploy an FK
    /// referencing the other app's PRODUCTION schema.
    #[tokio::test]
    async fn deploy_existing_table_cross_app_relation_fk_references_production() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let holder = "dtxfkhold";
        let other = "dtxfkother";
        let holder_id = ensure_app(&state, holder).await;
        let (vid, vname) = create_isolated_version(&state, holder_id).await;
        let vslug = slugify(&vname);
        let pool = &*state.database_pool;

        // Second app linked to production (so its production schema exists)
        // and to the version, owning the referenced collection.
        let other_id = ensure_app(&state, other).await;
        AppsService::new(&state)
            .link_apps_to_version(&[(other_id, vid)])
            .await
            .unwrap();
        migrate_schema(pool, other, &vname).await;
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        create_collection(
            &state,
            &test_ctx(other, PRODUCTION_VERSION),
            CreateCollectionRequest {
                name: "othercoll".to_string(),
                display_name: None,
                fields: vec![text_field("label")],
            },
        )
        .await
        .unwrap();
        create_collection(
            &state,
            &test_ctx(other, &vname),
            CreateCollectionRequest {
                name: "othercoll".to_string(),
                display_name: None,
                fields: vec![text_field("label")],
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();

        // Add a cross-app relation field to the holder's EXISTING notes table.
        let holder_ver_schema = format!("{holder}010{vslug}");
        update_collection(
            &state,
            &test_ctx(holder, &vname),
            "notes",
            UpdateCollectionRequest {
                fields: vec![
                    text_field("title"),
                    FieldDefinition {
                        name: "other_ref".to_string(),
                        field_type: "relationship".to_string(),
                        relationship_type: Some("many_to_one".to_string()),
                        related_app: Some(other.to_string()),
                        related_collection: Some("othercoll".to_string()),
                        ..Default::default()
                    },
                ],
                removed_fields: vec![],
                display_name: None,
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        let ver_fk = fk_def_for(pool, &holder_ver_schema, "notes", "other_ref")
            .await
            .expect("version schema must have the cross-app FK");
        assert!(
            ver_fk.contains(&format!("{other}010{vslug}.")),
            "version FK should reference the other app's version schema: {ver_fk}"
        );
        let field_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{holder_ver_schema}\".alcedocore_fields WHERE api_name = 'other_ref'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let notes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{holder_ver_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        insert_sys_log(
            &state,
            &holder_ver_schema,
            "item_created",
            "alcedocore_fields",
            json!({ "id": field_id, "api_name": "other_ref", "collection_id": notes_id }),
        )
        .await;

        let prod_schema = format!("{holder}010production");
        assert!(fk_def_for(pool, &prod_schema, "notes", "other_ref").await.is_none());

        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Overwrite))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));

        let prod_fk = fk_def_for(pool, &prod_schema, "notes", "other_ref")
            .await
            .expect("deploy must add the cross-app FK to the holder's production table");
        assert!(
            prod_fk.contains(&format!("{other}010production.othercoll")),
            "prod FK must reference the other app's production schema: {prod_fk}"
        );
        assert!(
            !prod_fk.contains(&format!("010{vslug}.")),
            "prod FK must not reference the version schema: {prod_fk}"
        );

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, other).await;
        cleanup_app(&state, holder).await;
    }

    /// A field TYPE CHANGE on an existing collection cannot be applied
    /// physically by a deploy (columns are only added, never altered) — the
    /// deploy must be refused up front, before anything is written, and
    /// production must stay untouched. Checked in wholesale mode, selected
    /// mode and dry_run.
    #[tokio::test]
    async fn deploy_refuses_field_type_change_before_writing() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtftc";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));
        // Change the type of the existing `title` field in the version
        // (string → int): drop + re-add of the physical column in the version.
        update_collection(
            &state,
            &test_ctx(app, &vname),
            "notes",
            UpdateCollectionRequest {
                fields: vec![FieldDefinition {
                    name: "title".to_string(),
                    field_type: "int".to_string(),
                    ..Default::default()
                }],
                removed_fields: vec![],
                display_name: None,
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        // Record the edit (the test app state's log channel is not drained).
        let (_fid, fmeta): (i32, Value) = sqlx::query_as(&format!(
            "SELECT id, to_jsonb(f) FROM \"{ver_schema}\".alcedocore_fields f WHERE api_name = 'title'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let edit_id = insert_sys_log(&state, &ver_schema, "item_updated", "alcedocore_fields", fmeta)
            .await;

        // Wholesale deploy must be refused.
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(!resp.ok, "deploy with a field type change must be refused");
        assert_eq!(resp.data["deployed"], json!(false));
        let unsup = resp.data["unsupported"].as_array().unwrap();
        assert!(
            unsup
                .iter()
                .any(|u| u["type"] == "field_type_change" && u["app"] == app),
            "unsupported list must contain field_type_change: {}",
            resp.data
        );
        // Production untouched: metadata row still string, column still there.
        let prod_type: String = sqlx::query_scalar(&format!(
            "SELECT options->>'type' FROM \"{prod_schema}\".alcedocore_fields WHERE api_name = 'title'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(prod_type, "string");
        assert!(column_exists(pool, &prod_schema, "notes", "title").await);

        // Selected mode with just that edit must be refused too.
        let resp2 = VersionsService::new(&state)
            .deploy(vid, &deploy_req_ids(vec![json!(edit_id)], OnConflict::Fail))
            .await
            .unwrap();
        assert!(!resp2.ok, "selected deploy with a field type change must be refused");
        assert!(
            resp2.data["unsupported"]
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["type"] == "field_type_change")
        );

        // dry_run reports it but stays ok (preview only).
        let resp3 = VersionsService::new(&state)
            .deploy(vid, &deploy_req(true, OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp3.ok);
        assert_eq!(resp3.data["deployed"], json!(false));
        assert!(!resp3.data["unsupported"].as_array().unwrap().is_empty());

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    /// A field DELETION on an existing collection cannot be applied physically
    /// by a deploy (columns are never dropped) — the deploy must be refused
    /// up front and production must keep the field row and the column.
    #[tokio::test]
    async fn deploy_refuses_field_deletion_before_writing() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtfdel";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));
        // Remove `title` in the version (add `body` so the collection is not empty).
        update_collection(
            &state,
            &test_ctx(app, &vname),
            "notes",
            UpdateCollectionRequest {
                fields: vec![text_field("body")],
                removed_fields: vec!["title".to_string()],
                display_name: None,
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        // The field row is gone in the version schema now.
        let gone: i64 = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{ver_schema}\".alcedocore_fields WHERE api_name = 'title'"
        ))
        .await;
        assert_eq!(gone, 0);
        // Record the deletion edit with the field's identity (collection_id +
        // api_name), as the real activity-log hook would.
        let (title_id, notes_id): (i32, i32) = sqlx::query_as(&format!(
            "SELECT f.id, f.collection_id FROM \"{prod_schema}\".alcedocore_fields f \
             WHERE f.api_name = 'title'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        insert_sys_log(
            &state,
            &ver_schema,
            "item_deleted",
            "alcedocore_fields",
            json!({ "id": title_id, "api_name": "title", "collection_id": notes_id }),
        )
        .await;

        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Fail))
            .await
            .unwrap();
        assert!(!resp.ok, "deploy with a field deletion must be refused");
        assert_eq!(resp.data["deployed"], json!(false));
        let unsup = resp.data["unsupported"].as_array().unwrap();
        assert!(
            unsup
                .iter()
                .any(|u| u["type"] == "field_deleted" && u["app"] == app),
            "unsupported list must contain field_deleted: {}",
            resp.data
        );
        // Production untouched: field row and physical column still there.
        let prod_fields: i64 = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_fields WHERE api_name = 'title'"
        ))
        .await;
        assert_eq!(prod_fields, 1);
        assert!(column_exists(pool, &prod_schema, "notes", "title").await);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn deploy_skip_reports_conflicts_and_overwrites_without_backup() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtskip";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        // A second collection that exists in BOTH prod and the version clone.
        create_collection(
            &state,
            &test_ctx(app, PRODUCTION_VERSION),
            CreateCollectionRequest {
                name: "extra".to_string(),
                display_name: None,
                fields: vec![text_field("label")],
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();

        let (vid, vname) = create_isolated_version(&state, app_id).await;
        let ver_schema = format!("{}010{}", app, slugify(&vname));

        let notes_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        let extra_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{ver_schema}\".alcedocore_collections WHERE \"table\" = 'extra'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();

        // Non-conflicting version edit: rename extra.
        let ctx = test_ctx(app, &vname);
        let collection = "alcedocore_collections".to_string();
        let mut svc = ItemsService::new(&state, &ctx, &collection);
        let mut q = Query::eq("name", json!("extra"));
        svc.update_items_by_query(&mut q, item_map! { "name" => "Extra Renamed" }, &mut None)
            .await
            .unwrap();
        let edit_extra = insert_sys_log(&state, &ver_schema, "collection_updated", "alcedocore_collections", json!({ "id": extra_id, "name": "extra" })).await;
        // Conflicting version edit: rename notes (prod changed it too).
        let mut q = Query::eq("name", json!("notes"));
        svc.update_items_by_query(&mut q, item_map! { "name" => "Skip Notes" }, &mut None)
            .await
            .unwrap();
        let edit_notes = insert_sys_log(&state, &ver_schema, "collection_updated", "alcedocore_collections", json!({ "id": notes_id, "name": "notes" })).await;
        sqlx::query(&format!(
            "INSERT INTO \"{prod_schema}\".alcedocore_system_logs (action, target, metadata) \
             VALUES ('collection_updated', 'alcedocore_collections', $1)"
        ))
        .bind(json!({ "id": notes_id, "name": "notes" }))
        .execute(pool)
        .await
        .unwrap();

        let start = chrono::Utc::now();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req(false, OnConflict::Skip))
            .await
            .unwrap();
        assert!(resp.ok, "skip should deploy: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));
        assert!(
            resp.data["conflicts"].as_array().unwrap().iter().any(|c| c["type"] == "changed_on_both_sides"),
            "expected the conflict to be reported: {}",
            resp.data
        );
        let skipped = resp.data["skipped"].as_array().unwrap();
        assert!(
            skipped.iter().any(|s| s["id"] == json!(edit_notes) && s["reason"] == "conflict with production"),
            "expected the conflicting edit in skipped: {}",
            resp.data
        );
        assert!(!skipped.iter().any(|s| s["id"] == json!(edit_extra)));

        // No backup exists, so skip mode cannot restore the conflicting prod
        // row: the wholesale overwrite wins and the conflict is only reported.
        let notes_name: (Option<String>,) = sqlx::query_as(&format!(
            "SELECT name FROM \"{prod_schema}\".alcedocore_collections WHERE \"table\" = 'notes'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(notes_name.0.as_deref(), Some("Skip Notes"));
        // … while the non-conflicting edit was deployed.
        let extra_name: (Option<String>,) = sqlx::query_as(&format!(
            "SELECT name FROM \"{prod_schema}\".alcedocore_collections WHERE \"table\" = 'extra'"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(extra_name.0.as_deref(), Some("Extra Renamed"));

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn overwrite_schema_batched_rolls_back_completely_on_failure() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtrollback";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;
        let prod_schema = format!("{}010production", app);

        let copy_tables = copy_set_tables(pool, &prod_schema).await.unwrap();
        let cols_before = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collections")).await;
        assert!(cols_before > 0);
        let fks_before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_constraint c \
             JOIN pg_class cl ON cl.oid = c.conrelid \
             JOIN pg_namespace n ON n.oid = cl.relnamespace \
             WHERE n.nspname = $1 AND c.contype = 'f'",
        )
        .bind(&prod_schema)
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(fks_before > 0);

        // A source schema that does not exist: the TRUNCATE and FK drops run
        // first, then the copy fails — the whole overwrite must roll back.
        let res = overwrite_schema_batched(
            (*pool).clone(),
            "nonexistent_schema_zzz".to_string(),
            prod_schema.clone(),
            copy_tables,
        )
        .await;
        assert!(res.is_err(), "overwrite with a missing source schema must fail");

        let cols_after = scalar_i64(&state, &format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_collections")).await;
        assert_eq!(cols_after, cols_before, "TRUNCATE must have been rolled back");
        let fks_after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_constraint c \
             JOIN pg_class cl ON cl.oid = c.conrelid \
             JOIN pg_namespace n ON n.oid = cl.relnamespace \
             WHERE n.nspname = $1 AND c.contype = 'f'",
        )
        .bind(&prod_schema)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(fks_after, fks_before, "dropped FKs must have been rolled back");

        cleanup_app(&state, app).await;
        let _ = app_id;
    }

    #[tokio::test]
    async fn deploy_twice_does_not_duplicate_plugin_installs_or_route_mappings() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtdedup";
        let app_id = ensure_app(&state, app).await;
        let pool = &*state.database_pool;

        let (vid, vname) = create_isolated_version(&state, app_id).await;

        let version_av_id: i32 = sqlx::query_scalar(
            "SELECT id FROM alcedocore.alcedocore_apps_versions WHERE app_id = $1 AND version_id = $2",
        )
        .bind(app_id)
        .bind(vid)
        .fetch_one(pool)
        .await
        .unwrap();
        let slug = format!("dedup-test-{}", &vname[3..]);
        let prefix = format!("/dd{}", &vname[3..]);
        let plugin_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins (slug, registry_id, plugin_type, image) \
             VALUES ($1, 1, 'dynamic', 'localhost:5000/deploy-test:1.0.0') RETURNING id",
        )
        .bind(&slug)
        .fetch_one(pool)
        .await
        .unwrap();
        let old_install_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins_installs \
             (plugin_id, app_version_id, plugin_version, enabled) \
             VALUES ($1, $2, '1.0.0', true) RETURNING id",
        )
        .bind(plugin_id)
        .bind(version_av_id)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO alcedocore.alcedocore_route_mappings (domain, path_prefix, install_id) \
             VALUES (NULL, $1, $2)",
        )
        .bind(&prefix)
        .bind(old_install_id)
        .execute(pool)
        .await
        .unwrap();

        let prod_av_id: i32 = sqlx::query_scalar(
            "SELECT av.id FROM alcedocore.alcedocore_apps_versions av \
             JOIN alcedocore.alcedocore_versions v ON v.id = av.version_id \
             WHERE av.app_id = $1 AND v.version_name = 'production'",
        )
        .bind(app_id)
        .fetch_one(pool)
        .await
        .unwrap();

        let start = chrono::Utc::now();
        for _ in 0..2 {
            let resp = VersionsService::new(&state)
                .deploy(vid, &deploy_req(false, OnConflict::Fail))
                .await
                .unwrap();
            assert!(resp.ok, "deploy should succeed: {}", resp.data);
        }

        // Exactly one install for (plugin_id, prod app_version_id) …
        let installs: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_plugins_installs \
             WHERE plugin_id = $1 AND app_version_id = $2",
        )
        .bind(plugin_id)
        .bind(prod_av_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(installs.0, 1, "re-deploy must not duplicate the install");
        // … and exactly one route mapping for the prefix (NULL domain included),
        // pointing at that install.
        let rms: Vec<(i32, i32)> = sqlx::query_as(
            "SELECT id, install_id FROM alcedocore.alcedocore_route_mappings WHERE path_prefix = $1",
        )
        .bind(&prefix)
        .fetch_all(pool)
        .await
        .unwrap();
        assert_eq!(rms.len(), 1, "re-deploy must not duplicate the route mapping");
        let install_id: i32 = sqlx::query_scalar(
            "SELECT id FROM alcedocore.alcedocore_plugins_installs \
             WHERE plugin_id = $1 AND app_version_id = $2",
        )
        .bind(plugin_id)
        .bind(prod_av_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(rms[0].1, install_id);

        let _ = sqlx::query("DELETE FROM alcedocore.alcedocore_route_mappings WHERE path_prefix = $1").bind(&prefix).execute(pool).await;
        let _ = sqlx::query("DELETE FROM alcedocore.alcedocore_plugins WHERE slug = $1").bind(&slug).execute(pool).await;
        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn concurrent_deploys_are_serialized_without_backups() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let app = "dtconc";
        let app_id = ensure_app(&state, app).await;

        let (vid, vname) = create_isolated_version(&state, app_id).await;

        let start = chrono::Utc::now();
        let req1 = deploy_req(false, OnConflict::Fail);
        let req2 = deploy_req(false, OnConflict::Fail);
        let svc1 = VersionsService::new(&state);
        let svc2 = VersionsService::new(&state);
        let (r1, r2) = tokio::join!(svc1.deploy(vid, &req1), svc2.deploy(vid, &req2));
        let r1 = r1.unwrap();
        let r2 = r2.unwrap();
        assert!(r1.ok && r2.ok, "both deploys should succeed: {} / {}", r1.data, r2.data);
        assert_eq!(r1.data["deployed"], json!(true));
        assert_eq!(r2.data["deployed"], json!(true));
        assert!(r1.data.get("backup_version").is_none() && r2.data.get("backup_version").is_none());
        assert_eq!(r1.data["warning"].as_str().unwrap(), NO_BACKUP_WARNING);
        let backups: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_versions WHERE version_name LIKE 'backup_before_deploy_%'",
        )
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert_eq!(backups.0, 0, "deploys must not create backup versions");

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, app).await;
    }

    #[tokio::test]
    async fn deploy_edit_ids_string_entry_selects_app_created_only() {
        let _guard = DEPLOY_TEST_LOCK.lock().await;
        let state = get_app_state().await;
        let holder = "dthold2";
        let holder_id = ensure_app(&state, holder).await;
        let new_app = "dtnew2";

        let (vid, vname) = create_isolated_version(&state, holder_id).await;

        let pool = &*state.database_pool;
        let system = AppContext::system(RequestSource::SystemTest);
        let collection = "alcedocore_apps".to_string();
        let mut svc = ItemsService::new(&state, &system, &collection);
        let ids = svc
            .create_many(
                vec![item_map! { "name" => new_app, "api_name" => new_app }],
                &mut None,
            )
            .await
            .unwrap();
        let new_app_id: i32 = ids[0].parse().unwrap();
        AppsService::new(&state)
            .link_apps_to_version(&[(new_app_id, vid)])
            .await
            .unwrap();
        migrate_schema(pool, new_app, &vname).await;
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();
        create_collection(
            &state,
            &test_ctx(new_app, &vname),
            CreateCollectionRequest {
                name: "newcoll".to_string(),
                display_name: None,
                fields: vec![text_field("label")],
            },
        )
        .await
        .unwrap();
        SchemaService::refresh_schema_and_meta(&state).await.unwrap();

        // Select only the new app via its api_name string (as the UI does);
        // the holder app has no selected edits and must not be written.
        let start = chrono::Utc::now();
        let resp = VersionsService::new(&state)
            .deploy(vid, &deploy_req_ids(vec![json!(new_app)], OnConflict::Fail))
            .await
            .unwrap();
        assert!(resp.ok, "deploy should succeed: {}", resp.data);
        assert_eq!(resp.data["deployed"], json!(true));
        let apps = resp.data["apps"].as_array().unwrap();
        assert!(
            apps.iter().any(|a| a["app"] == new_app && a["status"] == "created"),
            "expected the new app to be created: {}",
            resp.data
        );
        assert!(
            !apps.iter().any(|a| a["app"] == holder),
            "the holder app had no selected edits and must not be written: {}",
            resp.data
        );

        let prod_schema = format!("{}010production", new_app);
        let linked: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alcedocore.alcedocore_apps_versions av \
             JOIN alcedocore.alcedocore_versions v ON v.id = av.version_id \
             WHERE av.app_id = $1 AND v.version_name = 'production'",
        )
        .bind(new_app_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(linked.0, 1);
        let seeded_roles: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM \"{prod_schema}\".alcedocore_roles WHERE name IN ('admin', 'public')"
        ))
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(seeded_roles.0, 2);

        cleanup_version(&state, vid, &vname).await;
        cleanup_app(&state, new_app).await;
        cleanup_app(&state, holder).await;
    }
