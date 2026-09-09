use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::plugins::health::AppState;

/// Query parameters for the debug schema endpoint.
#[derive(Debug, Deserialize, Default)]
pub struct SchemaQueryParams {
    /// When true, re-run the inspector refresh against the live database
    /// before responding (otherwise the cached `CoreState.schema` is returned).
    #[serde(default)]
    pub refresh: bool,
}

/// GET /debug/schema
///
/// DEBUG ONLY — public (no auth), like `/health`. Returns the cached
/// inspector schema (`CoreState.schema`): tables, columns, and app versions.
/// Pass `?refresh=true` to re-scan the database first.
///
/// Remove or protect this before production: it exposes internal schema detail.
pub async fn get_schema(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SchemaQueryParams>,
) -> Result<impl IntoResponse, AppError> {
    if params.refresh {
        let fresh = alcedo_db::services::inspector::DatabaseSchema::new()
            .refresh(&state.core)
            .await;
        *state.core.schema.write().await = fresh.into();
    }
    let schema = state.core.schema.read().await.clone();
    Ok(Json(schema))
}
