use axum::{
    Json, Router,
    extract::State,
    routing::get,
};
use serde_json::{Value, json};

use crate::{
    AppState,
    services::{
        collections::ddl::GLOBAL_USERS_COLLECTION,
        context::{AppContext, ExtractContext, RequestSource},
        errors::AlcedoError,
        items::query::Query,
        permissions::read::{self, ReadAccess},
        respond::{JSendResponse, success},
    },
};

/// User options for the `user` field input. Users are not exposed through the
/// generic items API; this endpoint enforces the users collection's read policy
/// (resolved in the app context) and then reads the global table.
pub fn users_lookup_controller() -> Router<AppState> {
    Router::new().route("/users", get(list_users))
}

#[utoipa::path(get, path = "/api/app/lookup/users", tag = "Lookup",
    responses((status = OK, body = Value))
)]
async fn list_users(
    State(state): State<AppState>,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let access = read::resolve_read_access(
        &state,
        &context,
        GLOBAL_USERS_COLLECTION,
        context.identity.as_ref(),
    )
    .await?;

    if matches!(access, ReadAccess::Deny) {
        return Ok(Json(success(json!({ "data": [] }))));
    }

    // The physical table lives in the `alcedocore` schema; the policy was resolved
    // from the app schema (synthetic collection id -1).
    let mut system = AppContext::system(RequestSource::API);
    system.identity = context.identity.clone();

    let mut query = Query {
        fields: vec![
            "id".to_string(),
            "email".to_string(),
            "display_name".to_string(),
        ],
        limit: 50,
        ..Default::default()
    };
    query.access = access;

    let table = GLOBAL_USERS_COLLECTION.to_string();
    let rows = query.execute_query(&system, &state, &table).await?;
    Ok(Json(success(json!({ "data": rows }))))
}
