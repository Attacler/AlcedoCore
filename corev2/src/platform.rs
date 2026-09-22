use axum::{Router, routing::get};

use crate::{controllers, services::app_state::AppState};

pub fn platform_controller() -> Router<AppState> {
    return Router::new()
        .nest("/auth", controllers::auth::auth_controller())
        .nest("/users", controllers::users::users_controller())
        .nest("/sessions", controllers::sessions::sessions_controller())
        .nest("/apps", controllers::apps::apps_controller())
        .nest("/versions", controllers::versions::versions_controller())
        .nest(
            "/developer-keys",
            controllers::developer_keys::developer_keys_controller(),
        )
        .route("/me/apps", get(controllers::apps::me_apps))
        .nest("/settings", controllers::settings::settings_controller());
}
