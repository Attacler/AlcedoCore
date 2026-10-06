use axum::Router;

use crate::{controllers, services::app_state::AppState};

pub fn app_controller() -> Router<AppState> {
    return Router::new()
        .nest("/items", controllers::items::items_controller())
        .nest(
            "/collections",
            controllers::collections::tables_controller(),
        )
        .nest(
            "/settings",
            controllers::settings::app_settings_controller(),
        )
        .nest("/kv", controllers::kv::kv_controller())
        .nest("/logs", controllers::logs::logs_controller())
        .nest("/menus", controllers::menus::menus_controller())
        .nest("/files", controllers::files::files_controller())
        .nest("/roles", controllers::roles::roles_controller())
        .nest("/policies", controllers::policies::policies_controller())
        .nest("/users", controllers::roles::user_roles_controller())
        .nest("/lookup", controllers::users_lookup::users_lookup_controller());
}
