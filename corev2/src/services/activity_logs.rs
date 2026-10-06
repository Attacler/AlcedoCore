//! Three-tier activity logging.
//!
//! Every write records one row in the activity log that matches its scope:
//!
//! - **Global** actions (users, sessions, apps registry, system settings) go to
//!   `alcedocore.alcedocore_system_logs`.
//! - **App system** actions (system collections such as `alcedocore_roles`,
//!   `alcedocore_settings`) go to `{app}010{version}.alcedocore_system_logs`.
//! - **Collection** actions (normal user collections) go to
//!   `{app}010{version}.alcedocore_collection_logs`, carrying the item id and an
//!   optional field-level diff.
//!
//! Routing is driven purely by three inputs: the context's schema, whether the
//! caller passed a `collection_name`, and the collection's classification. A
//! collection whose name starts with `alcedocore_` is a *system*
//! collection and is logged as an app system row, never a collection row.

use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr, Value as SeaValue};
use serde_json::Value;
use sqlx::{Postgres, Transaction};

use crate::AppState;
use crate::middelware::auth::AuthLevel;
use crate::services::context::AppContext;
use crate::services::errors::AlcedoError;

/// Records a single activity-log row. See the module docs for the routing rule.
///
/// `diff` and `item_id` are only meaningful for collection logs; pass `None`
/// for system logs. `collection_name` is `Some` for collection logs and `None`
/// for system logs, and selects which app table is written.
///
/// When `tx` is `Some` the insert participates in the caller's transaction;
/// otherwise it runs directly on the pool.
#[allow(clippy::too_many_arguments)]
pub async fn record(
    state: &AppState,
    context: &AppContext,
    action: &str,
    target: &str,
    description: Option<String>,
    metadata: Value,
    diff: Option<Value>,
    collection_name: Option<&str>,
    item_id: Option<Value>,
    tx: Option<&mut Transaction<'_, Postgres>>,
) -> Result<(), AlcedoError> {
    let actor_id = actor_id(context);
    // Keep secrets out of the audit metadata (the item snapshot can hold
    // password/key hashes).
    let mut metadata = metadata;
    redact_metadata(target, &mut metadata);

    let request_id = context
        .request_id
        .clone()
        .or_else(crate::middelware::request_id::current_request_id);

    let schema = context.schema_name();

    let is_system_collection = collection_name.map_or(true, |name| is_system_collection_name(name));
    let use_collection_log = schema != "alcedocore" && !is_system_collection;

    // System tables share one column shape; collection logs add the item
    // columns. Values are rendered as escaped literals by `PostgresQueryBuilder`
    // (this crate's convention) rather than bound.
    let sql = if use_collection_log {
        Query::insert()
            .into_table((
                Alias::new(&schema),
                Alias::new("alcedocore_collection_logs"),
            ))
            .columns([
                Alias::new("actor_id"),
                Alias::new("action"),
                Alias::new("collection_name"),
                Alias::new("item_id"),
                Alias::new("diff"),
                Alias::new("metadata"),
                Alias::new("request_id"),
            ])
            .values_panic([
                uuid_expr(actor_id),
                Expr::value(action),
                Expr::value(collection_name.unwrap_or_default()),
                json_expr(Some(item_id.unwrap_or(Value::Null))),
                json_expr(diff),
                json_expr(Some(metadata)),
                text_expr(request_id),
            ])
            .to_string(PostgresQueryBuilder)
    } else {
        let table = if schema == "alcedocore" {
            "alcedocore_system_logs"
        } else {
            "alcedocore_system_logs"
        };
        Query::insert()
            .into_table((Alias::new(&schema), Alias::new(table)))
            .columns([
                Alias::new("actor_id"),
                Alias::new("action"),
                Alias::new("target"),
                Alias::new("description"),
                Alias::new("metadata"),
                Alias::new("request_id"),
            ])
            .values_panic([
                uuid_expr(actor_id),
                Expr::value(action),
                Expr::value(target),
                text_expr(description),
                json_expr(Some(metadata)),
                text_expr(request_id),
            ])
            .to_string(PostgresQueryBuilder)
    };

    let query = sqlx::query(&sql);
    match tx {
        Some(tx) => query.execute(&mut **tx).await?,
        None => query.execute(&*state.database_pool).await?,
    };

    Ok(())
}

/// A collection is a *system* collection when its name is prefixed with
/// `alcedocore_`; those are framework tables, not user data.
pub fn is_system_collection_name(name: &str) -> bool {
    name.starts_with("alcedocore_")
}

/// Maps a collection + CRUD verb to the activity action string the admin UI
/// renders. Unknown collections fall back to the generic item actions.
///
/// `verb` is one of `"created"`, `"updated"`, `"deleted"`.
pub fn action_for(collection: &str, verb: &str) -> String {
    match collection {
        "alcedocore_users" => match verb {
            "created" => "user_created",
            "deleted" => "user_deleted",
            _ => "user_updated",
        },
        "alcedocore_developer_api_keys" => match verb {
            "created" => "developer_key_created",
            "deleted" => "developer_key_deleted",
            _ => "developer_key_updated",
        },
        "alcedocore_sessions" => match verb {
            "created" => "login_success",
            "deleted" => "logout",
            _ => "login_success",
        },
        "alcedocore_settings" | "alcedocore_app_settings" => "setting_changed",
        "alcedocore_collections" => match verb {
            "created" => "collection_created",
            "deleted" => "collection_deleted",
            _ => "collection_updated",
        },
        "alcedocore_policies" => match verb {
            "created" => "policy_created",
            "deleted" => "policy_deleted",
            _ => "policy_updated",
        },
        "alcedocore_roles" => match verb {
            "created" => "role_created",
            "deleted" => "role_deleted",
            _ => "role_updated",
        },
        "alcedocore_user_roles" => match verb {
            "created" => "user_role_assigned",
            "deleted" => "user_role_removed",
            _ => "user_role_assigned",
        },
        _ => match verb {
            "created" => "item_created",
            "deleted" => "item_deleted",
            _ => "item_updated",
        },
    }
    .to_string()
}

/// A `uuid` value. sea-query's uuid support is not enabled, so render a string
/// literal and let Postgres coerce it to the column type.
fn uuid_expr(value: Option<uuid::Uuid>) -> SimpleExpr {
    Expr::value(SeaValue::String(value.map(|id| Box::new(id.to_string()))))
}

/// A nullable text value.
fn text_expr(value: Option<String>) -> SimpleExpr {
    Expr::value(SeaValue::String(value.map(Box::new)))
}

/// A nullable `jsonb` value.
fn json_expr(value: Option<Value>) -> SimpleExpr {
    Expr::value(SeaValue::Json(value.map(Box::new)))
}

/// The acting user id, when the caller is a session user.
pub fn actor_id(context: &AppContext) -> Option<uuid::Uuid> {
    match &context.identity {
        Some(AuthLevel::User(id)) => Some(*id),
        _ => None,
    }
}

/// Removes credential fields from the item snapshot before it is persisted as
/// audit metadata. `target` is the collection name.
fn redact_metadata(target: &str, metadata: &mut Value) {
    let secret = match target {
        "alcedocore_users" => Some("password_hash"),
        "alcedocore_developer_api_keys" => Some("key_hash"),
        _ => None,
    };
    if let (Some(field), Value::Object(map)) = (secret, metadata) {
        map.remove(field);
    }
}
