use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::services::app_state::AppState;

pub async fn load_schema(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(schema) = state.load_cached_schema_if_stale().await {
        *state.database_schema.write().await = schema;
    }
    next.run(req).await
}
