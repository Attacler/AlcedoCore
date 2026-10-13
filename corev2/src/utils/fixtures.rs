//! Idempotent test-environment builder.
//!
//! The permission integration tests assert against real Postgres: collections
//! with real columns and FKs, `alcedocore_collections` / `alcedocore_fields`
//! metadata, users, roles and policies. Historically all of that was created by
//! hand through the HTTP API, which made `cargo test` order-dependent and
//! meaningless on a fresh clone — a green run only proved the database happened
//! to hold the right rows.
//!
//! This module provisions the same environment through the same service calls
//! the API uses, so the suite is reproducible. It is safe to call repeatedly:
//! every step checks for what it needs first.
//!
//! Deliberately *not* a drop-and-recreate: tests share one database, and a
//! fixture that wiped it would race with every other test in the binary.

use sqlx::Row;
use uuid::Uuid;

use crate::{
    AppState,
    migrations::{
        app_migrations::run_app_migrations, system_migrations::run_system_migrations,
    },
    services::{
        apps::AppsService,
        collections::{
            CreateCollectionRequest, FieldDefinition, UpdateCollectionRequest,
            create_collection, schema::SchemaService, update_collection,
        },
        context::{AppContext, RequestSource},
        items::service::ItemsService,
        versions::service::VersionsService,
    },
};

/// App versions the permission tests build contexts for. Each becomes its own
/// `{app}010{version}` Postgres schema.
pub const APP_VERSIONS: &[(&str, &str)] = &[
    ("crm", "production"),
    ("helpdesk", "production"),
    ("testing", "production"),
];

const CRM: &str = "crm";
const HELPDESK: &str = "helpdesk";
const TESTING: &str = "testing";
const PRODUCTION: &str = "production";

/// The non-admin user the read-permission tests authenticate as.
pub const TEST_USER_EMAIL: &str = "customer@acme.example";

fn ctx(app: &str) -> AppContext {
    AppContext {
        app_name: app.to_string(),
        version: PRODUCTION.to_string(),
        request_source: RequestSource::SystemTest,
        identity: None,
        request_id: None,
    }
}

fn text(name: &str) -> FieldDefinition {
    FieldDefinition {
        name: name.to_string(),
        field_type: "string".to_string(),
        ..Default::default()
    }
}

fn fk(name: &str, collection: &str) -> FieldDefinition {
    FieldDefinition {
        name: name.to_string(),
        field_type: "relationship".to_string(),
        relationship_type: Some("many_to_one".to_string()),
        related_collection: Some(collection.to_string()),
        related_app: Some(CRM.to_string()),
        ..Default::default()
    }
}

/// `tickets.customer` reaches crm `customers` — the cross-app hop the
/// multi-hop filter test walks (`tickets.customer.contacts.first_name`).
fn cross_app_fk(name: &str, app: &str, collection: &str) -> FieldDefinition {
    FieldDefinition {
        related_app: Some(app.to_string()),
        ..fk(name, collection)
    }
}

// -------------------------------------------------------------- environment

/// Builds the whole test environment. Idempotent.
pub async fn ensure_test_env(state: &AppState) {
    run_system_migrations(&state.database_pool).await;

    // `ItemsService` resolves tables through the in-memory schema, so the
    // global tables have to be introspected before anything writes through it.
    // `main` does the same: refresh_schema() precedes ensure_default().
    state.refresh_schema().await;

    VersionsService::new(state)
        .ensure_default()
        .await
        .expect("fixture: could not ensure the default version");

    for (app, version) in APP_VERSIONS {
        ensure_app(state, app, version).await;
    }

    // App migrations only reach schemas whose app↔version link already exists,
    // so this must run after the loop above. The new schemas then need to be
    // introspected before collections can be declared inside them.
    run_app_migrations(&state.database_pool).await;

    // Load collection meta up front. On a re-run every collection already
    // exists, so the `ensure_*` helpers below skip creation entirely — and
    // `update_collection` plus the field validator both resolve through this
    // cache, so it has to be populated before they run.
    SchemaService::refresh_schema_and_meta(state).await;

    ensure_crm_collections(state).await;
    ensure_helpdesk_collections(state).await;
    ensure_testing_collection(state).await;

    seed_crm_rows(state).await;
    seed_helpdesk_rows(state).await;
    seed_customer_read_policy(state).await;

    crate::services::collections::schema::SchemaService::refresh_schema_and_meta(state).await;
}

async fn ensure_app(state: &AppState, app: &str, version: &str) {
    if AppsService::new(state).api_name_exists(app).await.unwrap_or(false) {
        return;
    }

    let version_id = VersionsService::new(state)
        .get_version_id_by_name(version)
        .await
        .unwrap_or_else(|e| panic!("fixture: lookup of version '{version}': {e}"))
        .unwrap_or_else(|| panic!("fixture: version '{version}' missing after ensure_default"));

    let system = AppContext::system(RequestSource::API);
    let apps_collection = "alcedocore_apps".to_string();
    let mut service = ItemsService::new(state, &system, &apps_collection);
    let ids = service
        .create_many(
            vec![crate::item_map! {
                "name" => app.to_string(),
                "api_name" => app.to_string(),
            }],
            &mut None,
        )
        .await
        .expect("fixture: could not create app");

    let app_id: i32 = ids
        .first()
        .and_then(|s| s.parse().ok())
        .expect("fixture: app id not returned");

    AppsService::new(state)
        .link_apps_to_version(&[(app_id, version_id)])
        .await
        .expect("fixture: could not link app to version");

    // `validate_related_apps` checks `app_versions` in the in-memory schema,
    // so the new link has to be visible before any collection declares a
    // cross-app relationship.
    state.refresh_schema().await;
}

/// Creates `collection` unless its metadata row already exists. Going through
/// `create_collection` (rather than raw DDL) is what keeps `alcedocore_collections`
/// and `alcedocore_fields` in step — the permission resolver reads both.
///
/// Reconciles at field level, not just collection level: a database seeded by
/// an older run may have the collection but not every field the tests expect,
/// and a collection-only check would silently leave it short.
async fn ensure_collection(
    state: &AppState,
    app: &str,
    name: &str,
    fields: Vec<FieldDefinition>,
) {
    if collection_id(state, app, name).await.is_none() {
        create_collection(
            state,
            &ctx(app),
            CreateCollectionRequest {
                name: name.to_string(),
                display_name: None,
                fields,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("fixture: could not create {app}.{name}: {e}"));
    } else {
        let missing = missing_fields(state, app, name, fields).await;
        if !missing.is_empty() {
            add_fields(state, app, name, missing).await;
            return;
        }
    }

    // A later collection may declare a FK to this one, and the validator
    // resolves related collections through the in-memory schema.
    SchemaService::refresh_schema_and_meta(state).await;
}

/// The subset of `declared` that `app.collection` does not already declare.
async fn missing_fields(
    state: &AppState,
    app: &str,
    name: &str,
    declared: Vec<FieldDefinition>,
) -> Vec<FieldDefinition> {
    let Ok(rows) = sqlx::query(&format!(
        "SELECT f.api_name FROM \"{schema}\".alcedocore_fields f
         JOIN \"{schema}\".alcedocore_collections c ON c.id = f.collection_id
         WHERE c.name = $1",
        schema = schema_of(app)
    ))
    .bind(name)
    .fetch_all(&*state.database_pool)
    .await
    else {
        // Without metadata we cannot tell what is missing; assume all of it so
        // the caller re-declares rather than silently skipping.
        return declared;
    };

    let present: Vec<String> = rows.iter().filter_map(|r| r.try_get(0).ok()).collect();
    declared
        .into_iter()
        .filter(|f| !present.contains(&f.name))
        .collect()
}

/// Adds fields to an existing collection, preserving the ones already declared.
/// Skips anything already present so it is safe to call on every run.
async fn add_fields(state: &AppState, app: &str, name: &str, fields: Vec<FieldDefinition>) {
    let fields = missing_fields(state, app, name, fields).await;
    if fields.is_empty() {
        return;
    }

    update_collection(
        state,
        &ctx(app),
        name,
        UpdateCollectionRequest {
            fields,
            removed_fields: vec![],
            display_name: None,
        },
    )
    .await
    .unwrap_or_else(|e| panic!("fixture: could not add fields to {app}.{name}: {e}"));

    // `update_collection` resolves the target through the collection *meta*
    // cache, not just the table list, so the new field needs a meta refresh to
    // be visible to what follows.
    SchemaService::refresh_schema_and_meta(state).await;
}

pub async fn collection_id(state: &AppState, app: &str, name: &str) -> Option<i64> {
    let id: Option<i32> = sqlx::query_scalar(&format!(
        "SELECT id FROM \"{schema}\".alcedocore_collections WHERE name = $1",
        schema = schema_of(app)
    ))
    .bind(name)
    .fetch_optional(&*state.database_pool)
    .await
    .ok()
    .flatten();

    id.map(i64::from)
}

/// Mirrors `AppContext::schema_name` — the `{app}010{version}` convention.
fn schema_of(app: &str) -> String {
    format!("{}010{}", app, PRODUCTION)
}

async fn ensure_crm_collections(state: &AppState) {
    // `contacts`, `customers_users` and `orders` each hold an m:1 FK back to
    // `customers`, while `customers` needs virtual 1:M back-references to two of
    // them. The validator only accepts a relation to a collection that already
    // exists, so create the owning side first and attach the reverse links last.
    ensure_collection(
        state,
        CRM,
        "customers",
        vec![
            text("name"),
            text("email"),
            text("status"),
            text("company"),
        ],
    )
    .await;

    ensure_collection(
        state,
        CRM,
        "contacts",
        vec![
            text("first_name"),
            text("last_name"),
            text("full_name"),
            text("company"),
            fk("customer", "customers"),
        ],
    )
    .await;

    ensure_collection(
        state,
        CRM,
        "customers_users",
        vec![
            fk("customer", "customers"),
            FieldDefinition {
                name: "user".to_string(),
                field_type: "user".to_string(),
                related_collection: Some("alcedocore_users".to_string()),
                ..Default::default()
            },
        ],
    )
    .await;

    ensure_collection(
        state,
        CRM,
        "orders",
        vec![text("total"), fk("customer", "customers")],
    )
    .await;

    // Virtual 1:M back-references — resolved as EXISTS subqueries, so no column
    // and no join table is created for them. Both are needed for the traversal
    // tests (`customers.contacts.first_name`) and for the seeded read policy,
    // whose filter walks `customers` -> `customers_users` -> `user`.
    //
    // Added last, once every referenced collection exists: the validator
    // resolves relations against the live schema and rejects forward references.
    add_fields(
        state,
        CRM,
        "customers",
        vec![
            FieldDefinition {
                name: "contacts".to_string(),
                field_type: "relationship".to_string(),
                relationship_type: Some("one_to_many".to_string()),
                related_collection: Some("contacts".to_string()),
                related_app: Some(CRM.to_string()),
                ..Default::default()
            },
            FieldDefinition {
                name: "customers_users".to_string(),
                field_type: "relationship".to_string(),
                relationship_type: Some("one_to_many".to_string()),
                related_collection: Some("customers_users".to_string()),
                related_app: Some(CRM.to_string()),
                ..Default::default()
            },
        ],
    )
    .await;
}

async fn ensure_helpdesk_collections(state: &AppState) {
    ensure_collection(
        state,
        HELPDESK,
        "tickets",
        vec![
            text("subject"),
            text("status"),
            text("priority"),
            text("channel"),
            text("description"),
            text("assignee"),
            cross_app_fk("customer", CRM, "customers"),
        ],
    )
    .await;

    ensure_collection(
        state,
        HELPDESK,
        "products",
        vec![
            text("name"),
            text("price"),
            text("tag"),
            text("type"),
            text("billed_per"),
            text("one_off_price"),
            text("price_per_period"),
            cross_app_fk("customer", CRM, "customers"),
        ],
    )
    .await;

    ensure_collection(
        state,
        HELPDESK,
        "ticket_comments",
        vec![
            text("body"),
            FieldDefinition {
                name: "ticket".to_string(),
                field_type: "relationship".to_string(),
                relationship_type: Some("many_to_one".to_string()),
                related_collection: Some("tickets".to_string()),
                related_app: Some(HELPDESK.to_string()),
                ..Default::default()
            },
        ],
    )
    .await;
}

async fn ensure_testing_collection(state: &AppState) {
    ensure_collection(
        state,
        TESTING,
        "cases",
        vec![text("title"), text("state")],
    )
    .await;
}

// ---------------------------------------------------------------- seed rows

async fn customer_exists(state: &AppState, name: &str) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM crm010production.customers WHERE name = $1",
    )
    .bind(name)
    .fetch_one(&*state.database_pool)
    .await
    .unwrap_or(0)
        > 0
}

async fn seed_crm_rows(state: &AppState) {
    if !customer_exists(state, "Acme Corp").await {
        insert_item(
            state,
            CRM,
            "customers",
            vec![
                ("name", json!("Acme Corp")),
                ("email", json!("hello@acme.example")),
                ("status", json!("active")),
                ("company", json!("Acme Corp")),
            ],
        )
        .await;
    }

    // A second customer with no membership: the row-level filter tests assert
    // it is excluded, so it must exist to be excluded.
    if !customer_exists(state, "Globex").await {
        insert_item(
            state,
            CRM,
            "customers",
            vec![
                ("name", json!("Globex")),
                ("email", json!("hello@globex.example")),
                ("status", json!("inactive")),
                ("company", json!("Globex Inc")),
            ],
        )
        .await;
    }

    let acme = customer_id(state, "Acme Corp").await;
    let globex = customer_id(state, "Globex").await;

    seed_contact(state, "Alice", "Anderson", acme).await;
    seed_contact(state, "Bob", "Brown", globex).await;
    seed_user_and_membership(state, acme).await;
}

async fn seed_contact(state: &AppState, first: &str, last: &str, customer: Option<Uuid>) {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM crm010production.contacts WHERE first_name = $1",
    )
    .bind(first)
    .fetch_one(&*state.database_pool)
    .await
    .unwrap_or(0);
    if exists > 0 {
        return;
    }

    insert_item(
        state,
        CRM,
        "contacts",
        vec![
            ("first_name", json!(first)),
            ("last_name", json!(last)),
            ("full_name", json!(format!("{first} {last}"))),
            (
                "company",
                json!(if first == "Alice" { "Acme Corp" } else { "Globex Inc" }),
            ),
            (
                "customer",
                match customer {
                    Some(id) => json!(id.to_string()),
                    None => Value::Null,
                },
            ),
        ],
    )
    .await;
}

async fn seed_helpdesk_rows(state: &AppState) {
    let acme = customer_id(state, "Acme Corp").await;

    for (subject, status, priority, channel) in [
        ("Acme login broken", "open", "high", "email"),
        ("Globex billing", "closed", "low", "phone"),
    ] {
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM helpdesk010production.tickets WHERE subject = $1")
                .bind(subject)
                .fetch_one(&*state.database_pool)
                .await
                .unwrap_or(0);
        if exists > 0 {
            continue;
        }
        let customer = if subject.starts_with("Acme") { acme } else { None };
        insert_item(
            state,
            HELPDESK,
            "tickets",
            vec![
                ("subject", json!(subject)),
                ("status", json!(status)),
                ("priority", json!(priority)),
                ("channel", json!(channel)),
                (
                    "customer",
                    match customer {
                        Some(id) => json!(id.to_string()),
                        None => Value::Null,
                    },
                ),
            ],
        )
        .await;
    }
}

// ------------------------------------------------------------- policy wiring

/// Seeds the read policy the `read` tests depend on: a role-bound policy
/// granting `read` on `customers`, filtered to rows the caller is a member of
/// via `customers_users.user = {user.id}`.
///
/// Without it every read resolves to `Deny` rather than `Restricted`, which is
/// what these tests are actually asserting against.
async fn seed_customer_read_policy(state: &AppState) {
    let s = "crm010production";

    let already: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM \"{s}\".alcedocore_policies WHERE name = $1"
    ))
    .bind("fixture-customer-read")
    .fetch_one(&*state.database_pool)
    .await
    .unwrap_or(0);
    if already > 0 {
        return;
    }

    let Some(customers_id) = collection_id(state, CRM, "customers").await else {
        panic!("fixture: crm.customers missing, cannot seed read policy");
    };

    let user_id = test_user_id(state).await;
    let role_id: Uuid = Uuid::new_v4();
    let policy_id: Uuid = Uuid::new_v4();

    sqlx::query(&format!(
        "INSERT INTO \"{s}\".alcedocore_roles (id, name, description, is_system) \
         VALUES ($1, $2, '', false)"
    ))
    .bind(role_id)
    .bind("fixture-customer")
    .execute(&*state.database_pool)
    .await
    .expect("fixture: insert role");

    sqlx::query(&format!(
        "INSERT INTO \"{s}\".alcedocore_user_roles (id, user_id, role_id) VALUES ($1, $2, $3)"
    ))
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(role_id)
    .execute(&*state.database_pool)
    .await
    .expect("fixture: insert user role");

    sqlx::query(&format!(
        "INSERT INTO \"{s}\".alcedocore_policies (id, name, description) VALUES ($1, $2, '')"
    ))
    .bind(policy_id)
    .bind("fixture-customer-read")
    .execute(&*state.database_pool)
    .await
    .expect("fixture: insert policy");

    sqlx::query(&format!(
        "INSERT INTO \"{s}\".alcedocore_role_policies (id, role_id, policy_id) \
         VALUES ($1, $2, $3)"
    ))
    .bind(Uuid::new_v4())
    .bind(role_id)
    .bind(policy_id)
    .execute(&*state.database_pool)
    .await
    .expect("fixture: insert role policy");

    // Row filter: keep only customers the caller belongs to. `fields` stays
    // empty so every column is readable; the tests assert masking separately
    // by driving `ReadAccess::Restricted` directly.
    sqlx::query(&format!(
        "INSERT INTO \"{s}\".alcedocore_policy_permissions \
         (id, policy_id, collection, action, fields, filter, field_validation) \
         VALUES ($1, $2, $3, 'read', '[]'::json, $4::json, '[]'::json)"
    ))
    .bind(Uuid::new_v4())
    .bind(policy_id)
    .bind(customers_id as i32)
    .bind(
        serde_json::to_string(&serde_json::json!([
            { "field": "customers_users.user", "operator": "eq", "value": "{user.id}" }
        ]))
        .unwrap(),
    )
    .execute(&*state.database_pool)
    .await
    .expect("fixture: insert policy permission");

    // Seeding bypasses `ItemsService`, so the permission cache would keep
    // serving the pre-seed answer ("Deny") for the rest of the run.
    crate::services::permissions::cache::invalidate_schema(state, s).await;
}

pub async fn test_user_id(state: &AppState) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM alcedocore.alcedocore_users WHERE email = $1",
    )
    .bind(TEST_USER_EMAIL)
    .fetch_one(&*state.database_pool)
    .await
    .expect("fixture: test user missing")
}

async fn seed_user_and_membership(state: &AppState, acme: Option<Uuid>) {
    let user_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM alcedocore.alcedocore_users WHERE email = $1",
    )
    .bind(TEST_USER_EMAIL)
    .fetch_optional(&*state.database_pool)
    .await
    .ok()
    .flatten();

    let user_id = match user_id {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO alcedocore.alcedocore_users (id, email, password_hash, is_admin) \
                 VALUES ($1, $2, 'x', false)",
            )
            .bind(id)
            .bind(TEST_USER_EMAIL)
            .execute(&*state.database_pool)
            .await
            .expect("fixture: insert test user");
            id
        }
    };

    let Some(customer) = acme else { return };

    // Bind both sides as UUIDs. Binding a `String` here makes Postgres compare
    // `uuid = text`, which errors; swallowing that would report "no membership"
    // on every run and insert a duplicate each time.
    let linked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM crm010production.customers_users \
         WHERE \"user\" = $1 AND customer = $2",
    )
    .bind(user_id)
    .bind(customer)
    .fetch_one(&*state.database_pool)
    .await
    .unwrap_or(0);
    if linked > 0 {
        return;
    }

    insert_item(
        state,
        CRM,
        "customers_users",
        vec![
            ("customer", json!(customer.to_string())),
            ("user", json!(user_id.to_string())),
        ],
    )
    .await;
}

// ------------------------------------------------------------------ helpers

use serde_json::{Value, json};

/// `customers.id` is a UUID, so this must decode as one — reading it as an
/// integer fails and silently yields `None`, which then seeds every foreign key
/// as NULL.
async fn customer_id(state: &AppState, name: &str) -> Option<Uuid> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM crm010production.customers WHERE name = $1")
        .bind(name)
        .fetch_optional(&*state.database_pool)
        .await
        .ok()
        .flatten()
}

/// Inserts a row through `ItemsService` so hooks (timestamps, validation) run
/// the same way they do for a real API write.
async fn insert_item(
    state: &AppState,
    app: &str,
    collection: &str,
    fields: Vec<(&str, Value)>,
) {
    let payload: serde_json::Map<String, Value> = fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

    let collection_name = collection.to_string();
    ItemsService::new(state, &ctx(app), &collection_name)
        .create_many(vec![payload], &mut None)
        .await
        .unwrap_or_else(|e| panic!("fixture: could not insert into {app}.{collection}: {e}"));
}

