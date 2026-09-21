use axum::Router;

use crate::{controllers, services::app_state::AppState};

pub fn app_controller() -> Router<AppState> {
    return Router::new()
        .nest("/items", controllers::items::items_controller())
        .nest(
            "/collections",
            controllers::collections::tables_controller(),
        )
        .nest("/settings", controllers::settings::settings_controller());
}
