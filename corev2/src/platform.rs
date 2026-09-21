use axum::Router;

use crate::{controllers, services::app_state::AppState};

pub fn platform_controller() -> Router<AppState> {
    return Router::new()
        .nest("/auth", controllers::auth::auth_controller())
        .nest("/apps", controllers::apps::apps_controller())
        .nest("/settings", controllers::settings::settings_controller());
}
