//! End-to-end integration test for the apps API (Backend Task 11).
//!
//! Proves, against a real Postgres:
//!   (a) the core boots and seeds only a `production` version (no app), and no
//!       app schema exists until an app is created;
//!   (b) an admin can create an app, which creates its schema
//!       (`<api_name>010<version>`) and seeds the `admin`/`public` roles;
//!   (c) two apps have isolated collection/item data via `X-App`/`X-Version`;
//!   (d) `/api/me/apps` and the user app-access endpoints work.
//!
//! The HTTP harness mirrors `common::setup()` but additionally wires a real
//! `db_url` onto `CoreState` (required for the lazily-created per-app-version
//! pools) and runs the real migration pipeline so the `production` version is
//! seeded exactly as it is at boot.

#[path = "common/mod.rs"]
mod common;

use std::env;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use axum::http::StatusCode;
use serde_json::{json, Value};
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;

const DEV_AUTH: &str = "Bearer dev_test-key-for-tests-12345";

/// `run_system_migrations` / `run_app_migrations_with_dir` read the
/// process-global `DATABASE_URL` and `CORE_MIGRATIONS_DIR`, so tests that set
/// them must run one at a time within this binary.
static ENV_LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();

async fn lock_env() -> OwnedMutexGuard<()> {
    ENV_LOCK
        .get_or_init(|| Arc::new(Mutex::new(())))
        .clone()
        .lock_owned()
        .await
}

struct Harness {
    server: axum_test::TestServer,
    pool: sqlx::PgPool,
    _container: ContainerAsync<Postgres>,
    _redis: common::TestRedis,
    _env_guard: OwnedMutexGuard<()>,
}

/// Boot a fresh Postgres, run the real system + app migrations, and build the
/// HTTP router with per-schema pool support enabled.
async fn boot() -> Harness {
    let env_guard = lock_env().await;

    let (pool, container) = common::start_postgres()
        .await
        .expect("Failed to start Postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);

    // `m00001_init` and `ensure_app_version_schema` resolve these from the env.
    env::set_var("DATABASE_URL", &url);
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core-migrations");
    env::set_var("CORE_MIGRATIONS_DIR", &migrations_dir);

    plugin_core::run_system_migrations(&pool)
        .await
        .expect("system migrations failed");
    plugin_core::run_app_migrations_with_dir(
        &pool,
        plugin_core::AppConfig::default(),
        migrations_dir.clone(),
    )
    .await
    .expect("app migrations failed");

    // The global/default zone. `common::TestDb` (used by the rest of the suite)
    // pre-creates the `default010v1` app schema, which hosts the system
    // collections (`alcedo_users`, ...) that the global pool resolves through
    // its `search_path`. Reproduce that here so `/api/users` works. This does
    // NOT register an app row, so the "no app seeded" assertion still holds.
    plugin_core::db::schema_migration::SchemaMigrationRunner::new(
        pool.clone(),
        "default010v1".to_string(),
        migrations_dir,
    )
    .apply_pending()
    .await
    .expect("default schema migration failed");

    // Redis backs the collection/permission caches; use a dedicated container
    // so the per-schema cache regression below genuinely exercises the cache.
    let redis = common::TestRedis::new().await.expect("Failed to start Redis");
    let mut state = common::create_test_state_full(pool.clone(), &redis.url).await;
    // Per-app-version pools are created lazily from this URL.
    state.core.db_url = Some(url.clone());

    // Boot mirrors production: populate the schema cache from the migrated
    // database so schema-aware services (ItemsService/TableShape) can resolve
    // global `alcedo.*` tables before any app handler rebuilds the cache.
    common::refresh_schema(&state).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        &pool,
        Some(common::DEV_API_KEY.to_string()),
    )
    .await;

    let session_layer = common::create_test_session_layer_at(&redis.url).await;
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(common::with_default_app_headers(app)).expect("Failed to create test server");

    Harness {
        server,
        pool,
        _container: container,
        _redis: redis,
        _env_guard: env_guard,
    }
}

fn body(resp: &axum_test::TestResponse) -> Value {
    serde_json::from_str(&resp.text()).unwrap_or_else(|_| {
        panic!(
            "response is not valid JSON (status {}): {}",
            resp.status_code(),
            resp.text()
        )
    })
}

async fn create_user(
    server: &axum_test::TestServer,
    email: &str,
    password: &str,
    is_admin: bool,
) -> Uuid {
    let resp = server
        .post("/api/users")
        .add_header("Authorization", DEV_AUTH)
        .json(&json!({ "email": email, "password": password, "is_admin": is_admin }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "create user failed: {}",
        resp.text()
    );
    let b = body(&resp);
    Uuid::parse_str(b["data"]["id"].as_str().expect("created user id")).expect("valid uuid")
}

/// Extract the live `alcedo_session=<id>` cookie from a login response.
fn extract_session_cookie(resp: &axum_test::TestResponse) -> String {
    let mut found: Option<String> = None;
    for value in resp.headers().get_all("set-cookie") {
        for part in value.to_str().unwrap_or_default().split(';') {
            let part = part.trim();
            if let Some(cookie) = part.strip_prefix("alcedo_session=") {
                if !cookie.is_empty() {
                    found = Some(part.to_string());
                }
            }
        }
    }
    found.expect("login response should set a non-empty alcedo_session cookie")
}

async fn login(server: &axum_test::TestServer, email: &str, password: &str) -> String {
    let resp = server
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": password }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "login failed: {}",
        resp.text()
    );
    extract_session_cookie(&resp)
}

async fn create_app(server: &axum_test::TestServer, name: &str, api_name: &str) -> Value {
    let resp = server
        .post("/api/apps")
        .add_header("Authorization", DEV_AUTH)
        .json(&json!({ "name": name, "api_name": api_name, "version": "production" }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "create app '{}' failed: {}",
        api_name,
        resp.text()
    );
    body(&resp)
}

#[tokio::test]
async fn apps_api_end_to_end() {
    let h = boot().await;
    let server = &h.server;
    let pool = &h.pool;

    // -------------------------------------------------------------------
    // (a) Boot seeding: only the `production` version, no app, no schema.
    // -------------------------------------------------------------------
    // Migrations seed only the `production` version — no app. The single
    // `default` app here comes from the harness's version-scoped dev-key
    // provisioning (`ensure_default_app_version`), not from boot.
    let app_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM alcedo.alcedo_apps")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        app_count, 1,
        "only the harness-provisioned default app should exist at boot"
    );
    let only_default: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM alcedo.alcedo_apps WHERE api_name <> 'default')",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(only_default, "no non-default app should be seeded at boot");

    let prod_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alcedo.alcedo_versions WHERE version_name = 'production'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        prod_count, 1,
        "exactly one production version should be seeded"
    );

    let pre_schema: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'shop010production')",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(
        !pre_schema,
        "no app schema should exist before app creation"
    );

    // -------------------------------------------------------------------
    // (b) An admin creates two apps; each gets a schema + seeded roles.
    // -------------------------------------------------------------------
    let admin_email = format!("admin-{}@test.com", Uuid::new_v4());
    let admin_id = create_user(server, &admin_email, "admin-pass-123", true).await;
    let admin_cookie = login(server, &admin_email, "admin-pass-123").await;
    assert!(!admin_id.is_nil());

    let shop = create_app(server, "shop", "shop").await;
    assert_eq!(shop["data"]["api_name"].as_str(), Some("shop"));
    assert!(
        shop["data"]["versions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("production")),
        "shop should be attached to production: {}",
        shop
    );

    let shop_schema_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'shop010production')",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(shop_schema_exists, "shop010production schema must exist");

    let shop_admin_role: Uuid = sqlx::query_scalar(
        "SELECT id FROM shop010production.alcedocore_roles WHERE name = 'admin'",
    )
    .fetch_one(pool)
    .await
    .expect("shop admin role should be seeded");
    let shop_public_roles: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM shop010production.alcedocore_roles WHERE name = 'public'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(shop_public_roles, 1, "shop public role should be seeded");
    let shop_admin_scopes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM shop010production.alcedocore_role_scopes WHERE role_id = $1",
    )
    .bind(shop_admin_role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(shop_admin_scopes > 0, "shop admin role should have scopes");

    // -------------------------------------------------------------------
    // (b2) Regression: a global-admin *session* (not a dev key) making a
    // scope-gated write in an app context must succeed. The auth middleware
    // scope gate must resolve the app-context pool and bypass global admins
    // (previously it checked scopes on the default pool and 4xx'd).
    // -------------------------------------------------------------------
    let admin_coll = format!("adm_{}", &Uuid::new_v4().to_string().replace('-', "")[..8]);
    let resp = server
        .post("/api/collections")
        .add_header("cookie", &admin_cookie)
        .add_header("X-App", "shop")
        .add_header("X-Version", "production")
        .json(&json!({ "name": admin_coll, "fields": [{ "name": "title", "type": "string" }] }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::CREATED,
        "global-admin session should create a collection in an app context: {}",
        resp.text()
    );

    let billing = create_app(server, "billing", "billing").await;
    assert_eq!(billing["data"]["api_name"].as_str(), Some("billing"));
    let billing_schema_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'billing010production')",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(
        billing_schema_exists,
        "billing010production schema must exist"
    );

    // -------------------------------------------------------------------
    // (c) Data isolation: a collection/item in `shop` is invisible to `billing`.
    // -------------------------------------------------------------------
    let coll = format!("iso_{}", &Uuid::new_v4().to_string().replace('-', "")[..8]);

    let resp = server
        .post("/api/collections")
        .add_header("cookie", &admin_cookie)
        .add_header("X-App", "shop")
        .add_header("X-Version", "production")
        .json(&json!({ "name": coll, "fields": [{ "name": "title", "type": "string" }] }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::CREATED,
        "create collection in shop failed: {}",
        resp.text()
    );

    // Regression: a global admin (`alcedo_users.is_admin = true`) with an app
    // context must see the app's collections via `GET /api/collections`.
    // Previously the admin check only consulted the app-scoped role tables
    // (empty for a global admin), so the list came back empty.
    let resp = server
        .get("/api/collections")
        .add_header("cookie", &admin_cookie)
        .add_header("X-App", "shop")
        .add_header("X-Version", "production")
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "global-admin collection list failed: {}",
        resp.text()
    );
    let admin_coll_names: Vec<String> = body(&resp)["collections"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        admin_coll_names.iter().any(|n| n == &coll),
        "global admin must see shop collection '{}', got: {:?}",
        coll,
        admin_coll_names
    );
    assert!(
        admin_coll_names.iter().any(|n| n == &admin_coll),
        "global admin must see shop collection '{}', got: {:?}",
        admin_coll,
        admin_coll_names
    );

    let resp = server
        .post(&format!("/api/items/{}", coll))
        .add_header("cookie", &admin_cookie)
        .add_header("X-App", "shop")
        .add_header("X-Version", "production")
        .json(&json!({ "title": "shop-only" }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "create item in shop failed: {}",
        resp.text()
    );

    let resp = server
        .get(&format!("/api/items/{}", coll))
        .add_header("cookie", &admin_cookie)
        .add_header("X-App", "shop")
        .add_header("X-Version", "production")
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "list shop items failed");
    let shop_items = body(&resp)["items"].as_array().cloned().unwrap_or_default();
    assert_eq!(shop_items.len(), 1, "shop should see its own item");

    let resp = server
        .get(&format!("/api/items/{}", coll))
        .add_header("cookie", &admin_cookie)
        .add_header("X-App", "billing")
        .add_header("X-Version", "production")
        .await;
    let billing_status = resp.status_code();
    if billing_status == StatusCode::OK {
        let billing_items = body(&resp)["items"].as_array().cloned().unwrap_or_default();
        assert!(
            billing_items.is_empty(),
            "billing must not see shop's item, got: {}",
            resp.text()
        );
    } else {
        assert_eq!(
            billing_status,
            StatusCode::NOT_FOUND,
            "billing should either be empty or 404: {}",
            resp.text()
        );
    }

    // -------------------------------------------------------------------
    // (d) /api/me/apps and user app-access endpoints.
    // -------------------------------------------------------------------
    // Admin sees both apps.
    let resp = server
        .get("/api/me/apps")
        .add_header("cookie", &admin_cookie)
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "me/apps failed: {}",
        resp.text()
    );
    let admin_apps = body(&resp)["data"].as_array().cloned().unwrap_or_default();
    let admin_names: Vec<&str> = admin_apps
        .iter()
        .filter_map(|a| a["api_name"].as_str())
        .collect();
    assert!(
        admin_names.contains(&"shop") && admin_names.contains(&"billing"),
        "admin should see both apps, got: {:?}",
        admin_names
    );

    // Grant a regular user the shop admin role, then verify the read paths.
    let user_email = format!("bob-{}@test.com", Uuid::new_v4());
    let user_id = create_user(server, &user_email, "bob-pass-123", false).await;

    // Regression: a non-admin (no app roles) must get 403 — not 500 — from the
    // admin-only apps API, even before any app role is granted.
    let bob_cookie = login(server, &user_email, "bob-pass-123").await;
    let resp = server
        .get("/api/apps")
        .add_header("cookie", &bob_cookie)
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::FORBIDDEN,
        "non-admin GET /api/apps should be 403, got {}: {}",
        resp.status_code(),
        resp.text()
    );

    let resp = server
        .put(&format!("/api/users/{}/app-access", user_id))
        .add_header("Authorization", DEV_AUTH)
        .json(&json!({
            "app": "shop",
            "version": "production",
            "role_ids": [shop_admin_role.to_string()],
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "set user app-access failed: {}",
        resp.text()
    );
    assert_eq!(body(&resp)["success"], json!(true));

    let resp = server
        .get(&format!("/api/users/{}/app-access", user_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "get user app-access failed: {}",
        resp.text()
    );
    let access = body(&resp)["data"].as_array().cloned().unwrap_or_default();
    let shop_entry = access
        .iter()
        .find(|a| a["api_name"].as_str() == Some("shop"))
        .expect("shop app-access entry should be present");
    assert!(
        shop_entry["roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r.as_str() == Some("admin")),
        "shop app-access should include the admin role: {}",
        shop_entry
    );

    // The same user's /api/me/apps shows shop only (no billing grant).
    let user_cookie = login(server, &user_email, "bob-pass-123").await;
    let resp = server
        .get("/api/me/apps")
        .add_header("cookie", user_cookie)
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "me/apps failed: {}",
        resp.text()
    );
    let user_apps = body(&resp)["data"].as_array().cloned().unwrap_or_default();
    let user_names: Vec<&str> = user_apps
        .iter()
        .filter_map(|a| a["api_name"].as_str())
        .collect();
    assert_eq!(
        user_names,
        vec!["shop"],
        "non-admin should see only the granted app, got: {:?}",
        user_names
    );

    // -------------------------------------------------------------------
    // (e) Regression: the collection-definition Redis cache must be scoped by
    // app-version schema. Two apps each define a `notes` collection with
    // different fields; after billing's `notes` (field `name`) is cached, a
    // request for shop's `notes` must still resolve shop's own definition
    // (field `title`). Previously the unscoped `schema:collection:notes` key
    // served billing's definition to shop, so the shop listing SELECTed a
    // `name` column that does not exist in shop's table (500).
    // -------------------------------------------------------------------
    for (app, field) in [("shop", "title"), ("billing", "name")] {
        let resp = server
            .post("/api/collections")
            .add_header("cookie", &admin_cookie)
            .add_header("X-App", app)
            .add_header("X-Version", "production")
            .json(&json!({ "name": "notes", "fields": [{ "name": field, "type": "string" }] }))
            .await;
        assert_eq!(
            resp.status_code(),
            StatusCode::CREATED,
            "create 'notes' in {} failed: {}",
            app,
            resp.text()
        );
    }

    for (app, field, val) in [
        ("shop", "title", "shop-note"),
        ("billing", "name", "billing-note"),
    ] {
        let resp = server
            .post("/api/items/notes")
            .add_header("cookie", &admin_cookie)
            .add_header("X-App", app)
            .add_header("X-Version", "production")
            .json(&json!({ field: val }))
            .await;
        assert_eq!(
            resp.status_code(),
            StatusCode::OK,
            "create item in {} 'notes' failed: {}",
            app,
            resp.text()
        );
    }

    // Fetch billing's `notes` first so the cache is populated with billing's
    // definition under billing's schema-scoped key.
    let resp = server
        .get("/api/items/notes")
        .add_header("cookie", &admin_cookie)
        .add_header("X-App", "billing")
        .add_header("X-Version", "production")
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "billing 'notes' listing failed: {}",
        resp.text()
    );
    let billing_items = body(&resp)["items"].as_array().cloned().unwrap_or_default();
    assert_eq!(billing_items.len(), 1, "billing should see its own note");
    assert!(
        billing_items[0].get("name").is_some(),
        "billing note should expose 'name', got: {}",
        billing_items[0]
    );

    // Now shop's listing must NOT reuse billing's cached definition.
    let resp = server
        .get("/api/items/notes")
        .add_header("cookie", &admin_cookie)
        .add_header("X-App", "shop")
        .add_header("X-Version", "production")
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "shop 'notes' listing must succeed with per-schema cache: {}",
        resp.text()
    );
    let shop_items = body(&resp)["items"].as_array().cloned().unwrap_or_default();
    assert_eq!(shop_items.len(), 1, "shop should see its own note");
    assert!(
        shop_items[0].get("title").is_some(),
        "shop note should expose 'title', got: {}",
        shop_items[0]
    );
    assert!(
        shop_items[0].get("name").is_none(),
        "shop note must not expose billing's 'name' field: {}",
        shop_items[0]
    );
}

#[tokio::test]
async fn test_get_app_version_access_lists_roles_and_users() {
    let h = boot().await;
    let server = &h.server;
    let app = create_app(server, "shop", "shop").await;
    let app_id = app["data"]["id"].as_i64().expect("app id") as i32;
    let version_id: i32 =
        sqlx::query_scalar("SELECT id FROM alcedo.alcedo_versions WHERE version_name = 'production'")
            .fetch_one(&h.pool)
            .await
            .expect("production version id");

    let resp = server
        .get(&format!("/api/apps/{}/versions/{}/access", app_id, version_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());
    let data = &body(&resp)["data"];
    assert_eq!(data["app"]["api_name"], json!("shop"));
    assert_eq!(data["version"]["version_name"], json!("production"));
    assert!(
        data["roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == json!("admin")),
        "roles should include the seeded admin role: {}",
        data["roles"]
    );
    assert_eq!(data["users"].as_array().unwrap().len(), 0);

    let resp = server
        .get(&format!("/api/apps/{}/versions/{}/access", app_id, 999999))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::NOT_FOUND, "{}", resp.text());

    // A user with two roles must appear as a single AccessUser carrying both
    // roles (grouping keyed by `user_id`, not email).
    let role_ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM shop010production.alcedocore_roles WHERE name IN ('admin', 'public')"#,
    )
    .fetch_all(&h.pool)
    .await
    .expect("shop should have seeded admin + public roles");
    assert_eq!(role_ids.len(), 2, "expected admin + public roles, got {role_ids:?}");

    let user_email = format!("group-{}@test.com", Uuid::new_v4());
    let user_id = create_user(server, &user_email, "group-pass-123", false).await;

    // Admin-only: a non-admin session (no app roles) gets 403.
    let cookie = login(server, &user_email, "group-pass-123").await;
    let resp = server
        .get(&format!("/api/apps/{}/versions/{}/access", app_id, version_id))
        .add_header("cookie", &cookie)
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::FORBIDDEN,
        "non-admin GET access should be 403, got {}: {}",
        resp.status_code(),
        resp.text()
    );

    // Grant both roles directly in the app schema. The `PUT .../access` write
    // endpoint is Task 2; seed rows here so this stays a GET-only test.
    for role_id in &role_ids {
        sqlx::query(
            r#"INSERT INTO shop010production.alcedocore_user_roles (user_id, role_id) VALUES ($1, $2)"#,
        )
        .bind(user_id)
        .bind(role_id)
        .execute(&h.pool)
        .await
        .expect("grant role in shop schema");
    }

    let resp = server
        .get(&format!("/api/apps/{}/versions/{}/access", app_id, version_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());
    let data = &body(&resp)["data"];
    let users = data["users"].as_array().unwrap();
    let matching: Vec<&Value> = users
        .iter()
        .filter(|u| u["user_id"] == json!(user_id.to_string()))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "user with two roles must appear exactly once: {}",
        data["users"]
    );
    let matched = matching[0];
    let mut role_names: Vec<String> = matched["role_names"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r.as_str().map(str::to_string))
        .collect();
    role_names.sort();
    assert_eq!(
        role_names,
        vec!["admin".to_string(), "public".to_string()],
        "both roles must be grouped onto the user: {}",
        matched
    );
    assert_eq!(
        matched["role_ids"].as_array().unwrap().len(),
        2,
        "role_ids should carry both roles: {}",
        matched
    );
}

#[tokio::test]
async fn test_set_and_revoke_app_version_access() {
    let h = boot().await;
    let server = &h.server;
    let app = create_app(server, "shop", "shop").await;
    let app_id = app["data"]["id"].as_i64().expect("app id") as i32;
    let version_id: i32 =
        sqlx::query_scalar("SELECT id FROM alcedo.alcedo_versions WHERE version_name = 'production'")
            .fetch_one(&h.pool)
            .await
            .expect("version id");
    let admin_role: Uuid =
        sqlx::query_scalar(r#"SELECT id FROM "shop010production".alcedocore_roles WHERE name = 'admin'"#)
            .fetch_one(&h.pool)
            .await
            .expect("admin role id");
    let user_email = format!("acc-{}@test.com", Uuid::new_v4());
    let user_id = create_user(server, &user_email, "pass-123456", false).await;

    // Grant.
    let resp = server
        .put(&format!("/api/apps/{}/versions/{}/access", app_id, version_id))
        .add_header("Authorization", DEV_AUTH)
        .json(&json!({ "user_id": user_id, "role_ids": [admin_role] }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());

    let resp = server
        .get(&format!("/api/apps/{}/versions/{}/access", app_id, version_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    let data = &body(&resp)["data"];
    let users = data["users"].as_array().unwrap();
    assert!(
        users.iter().any(|u| u["user_id"] == json!(user_id.to_string())
            && u["role_names"].as_array().unwrap().iter().any(|r| r == "admin")),
        "user should have the admin role: {}",
        data["users"]
    );

    // Unknown role id → 400.
    let resp = server
        .put(&format!("/api/apps/{}/versions/{}/access", app_id, version_id))
        .add_header("Authorization", DEV_AUTH)
        .json(&json!({ "user_id": user_id, "role_ids": [Uuid::new_v4()] }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::BAD_REQUEST, "{}", resp.text());

    // Revoke.
    let resp = server
        .delete(&format!("/api/apps/{}/versions/{}/access/{}", app_id, version_id, user_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());

    let resp = server
        .get(&format!("/api/apps/{}/versions/{}/access", app_id, version_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(
        body(&resp)["data"]["users"].as_array().unwrap().len(),
        0,
        "revoke should remove the user's roles"
    );
}

#[tokio::test]
async fn versions_api_round_trip() {
    let h = boot().await;
    let server = &h.server;

    // Seed: only `production` exists at boot.
    let resp = server
        .get("/api/versions")
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());
    let listed = body(&resp);
    let versions = listed["data"].as_array().expect("versions array");
    assert!(
        versions
            .iter()
            .any(|v| v["version_name"] == json!("production")),
        "seeded production version should be listed: {}",
        listed
    );
    let ids: Vec<i64> = versions.iter().filter_map(|v| v["id"].as_i64()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "versions must be sorted by id asc");

    // Create: the name is persisted slugified.
    let name = format!("Rel_{}", Uuid::new_v4().simple());
    let resp = server
        .post("/api/versions")
        .add_header("Authorization", DEV_AUTH)
        .json(&json!({ "version_name": name }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());
    let created = body(&resp);
    let created_id = created["data"]["id"].as_i64().expect("created version id");
    assert_eq!(
        created["data"]["version_name"].as_str(),
        Some(name.to_lowercase().as_str()),
        "version_name should be slugified: {}",
        created
    );
    assert!(created_id > 0);

    // A duplicate name is rejected by the unique index.
    let resp = server
        .post("/api/versions")
        .add_header("Authorization", DEV_AUTH)
        .json(&json!({ "version_name": name }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::CONFLICT, "{}", resp.text());

    // Whitespace-only and empty names are rejected at the boundary.
    for blank in ["   ", ""] {
        let resp = server
            .post("/api/versions")
            .add_header("Authorization", DEV_AUTH)
            .json(&json!({ "version_name": blank }))
            .await;
        assert_eq!(
            resp.status_code(),
            StatusCode::BAD_REQUEST,
            "blank version_name {:?} should be 400: {}",
            blank,
            resp.text()
        );
    }

    // Round-trip: the new version is visible in the list.
    let resp = server
        .get("/api/versions")
        .add_header("Authorization", DEV_AUTH)
        .await;
    let listed = body(&resp);
    assert!(
        listed["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == json!(created_id)),
        "created version should appear in the list: {}",
        listed
    );

    // Delete: succeeds and the version disappears.
    // The fold first clears `alcedo_developer_api_keys` (no ON DELETE CASCADE
    // on that FK), then app links, then the version itself — so a dev key
    // created for this version must be gone afterwards.
    let created_id_i32 = created_id as i32;
    let resp = server
        .post("/api/settings/developer/keys")
        .add_header("Authorization", DEV_AUTH)
        .json(&json!({ "name": "roundtrip-key", "version_id": created_id_i32 }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::OK,
        "create dev key for version failed: {}",
        resp.text()
    );
    let dev_key_count_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alcedo.alcedo_developer_api_keys WHERE version_id = $1",
    )
    .bind(created_id_i32)
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(
        dev_key_count_before, 1,
        "exactly one dev key should reference the version before delete"
    );

    let resp = server
        .delete(&format!("/api/versions/{}", created_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());
    assert_eq!(body(&resp)["success"], json!(true));

    let dev_key_count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alcedo.alcedo_developer_api_keys WHERE version_id = $1",
    )
    .bind(created_id_i32)
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(
        dev_key_count_after, 0,
        "deleting a version must remove its developer API keys (FK hazard fix)"
    );
    let av_count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alcedo.alcedo_apps_versions WHERE version_id = $1",
    )
    .bind(created_id_i32)
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(
        av_count_after, 0,
        "deleting a version must remove its app-version links"
    );

    let resp = server
        .get("/api/versions")
        .add_header("Authorization", DEV_AUTH)
        .await;
    let listed = body(&resp);
    assert!(
        !listed["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == json!(created_id)),
        "deleted version must not appear in the list: {}",
        listed
    );

    // Deleting a nonexistent version stays a 404.
    let resp = server
        .delete("/api/versions/999999")
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::NOT_FOUND, "{}", resp.text());
}

#[tokio::test]
async fn apps_list_and_get_return_engine_rows() {
    let h = boot().await;
    let server = &h.server;

    // Create two apps with distinct names so the list's sort order is
    // observable. The harness provisions a `default` app (version `v1`), so the
    // expected list is [alpha-app, default, zeta-app] sorted by name asc.
    let zeta = create_app(server, "zeta-app", "zeta").await;
    let alpha = create_app(server, "alpha-app", "alpha").await;
    let alpha_id = alpha["data"]["id"].as_i64().expect("alpha app id");
    let zeta_id = zeta["data"]["id"].as_i64().expect("zeta app id");

    // GET /api/apps (admin): every app exposes the AppWithVersions field set
    // and is ordered by name ascending.
    let resp = server
        .get("/api/apps")
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());
    let listed_body = body(&resp);
    let listed = listed_body["data"].as_array().expect("apps array");
    assert!(!listed.is_empty(), "at least the default app should be listed");
    for app in listed {
        for key in ["id", "name", "api_name", "icon", "logo", "versions"] {
            assert!(
                app.get(key).is_some(),
                "app row missing '{}': {}",
                key,
                app
            );
        }
        assert!(
            !app["versions"].as_array().unwrap().is_empty(),
            "app should carry at least one version: {}",
            app
        );
    }
    let names: Vec<&str> = listed.iter().filter_map(|a| a["name"].as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "apps must be sorted by name asc: {:?}", names);

    let alpha_entry = listed
        .iter()
        .find(|a| a["api_name"] == json!("alpha"))
        .expect("alpha app should be listed");
    assert_eq!(alpha_entry["id"], json!(alpha_id));
    assert_eq!(alpha_entry["versions"], json!(["production"]));
    assert!(
        listed
            .iter()
            .any(|a| a["api_name"] == json!("default") && a["versions"] == json!(["v1"])),
        "harness-provisioned default app (v1) should be listed: {:?}",
        listed
    );

    // GET /api/apps/:id (admin): the single app with its versions.
    let resp = server
        .get(&format!("/api/apps/{}", alpha_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());
    let got = &body(&resp)["data"];
    assert_eq!(got["id"], json!(alpha_id));
    assert_eq!(got["name"], json!("alpha-app"));
    assert_eq!(got["api_name"], json!("alpha"));
    assert_eq!(got["versions"], json!(["production"]));

    let resp = server
        .get(&format!("/api/apps/{}", zeta_id))
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "{}", resp.text());
    assert_eq!(body(&resp)["data"]["api_name"], json!("zeta"));

    // Unknown id → 404.
    let resp = server
        .get("/api/apps/999999")
        .add_header("Authorization", DEV_AUTH)
        .await;
    assert_eq!(resp.status_code(), StatusCode::NOT_FOUND, "{}", resp.text());
}
