use axum::{Router, routing::get};
use utoipa::OpenApi;

use crate::AppState;
use crate::controllers::items::DocsItemFilter;
use crate::controllers::{
    apps, auth, collections, developer_keys, items, policies, roles, sessions, settings, users,
    versions,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        // Items
        items::get_items,
        items::get_item,
        items::create_items,
        items::update_items,
        items::update_item,
        items::delete_items,
        items::get_references,
        // Collections
        collections::get_schema,
        collections::get_ts_schema,
        collections::list_collections,
        collections::get_collection,
        collections::create_collection,
        collections::update_collection,
        collections::delete_collection,
        collections::create_policy,
        collections::list_layouts,
        collections::create_layout,
        collections::update_layout,
        collections::delete_layout,
        collections::resolve_layout,
        collections::get_layout_roles,
        collections::set_layout_roles,
        collections::list_sections,
        collections::create_section,
        collections::update_section,
        collections::delete_section,
        collections::reorder_sections,
        // Settings
        settings::get_settings,
        // Auth
        auth::get_me,
        auth::login_handler,
        auth::logout_handler,
        // Users
        users::list_users,
        users::get_user,
        users::create_user,
        users::update_user,
        users::delete_user,
        users::change_password,
        users::get_user_sessions,
        users::revoke_user_session,
        users::revoke_all_user_sessions,
        // Sessions
        sessions::list_own_sessions,
        sessions::revoke_own_session,
        sessions::revoke_all_own_sessions,
        // Apps
        apps::list_apps,
        apps::get_app,
        apps::create_app,
        apps::update_app,
        apps::delete_app,
        apps::me_apps,
        // Versions
        versions::list_versions,
        versions::create_version,
        versions::delete_version,
        // Developer keys
        developer_keys::list_keys,
        developer_keys::create_key,
        developer_keys::delete_key,
        // Roles
        roles::list_roles,
        roles::create_role,
        roles::get_role,
        roles::update_role,
        roles::delete_role,
        roles::list_role_scopes,
        roles::set_role_scopes,
        roles::delete_role_scope,
        roles::list_role_policies,
        roles::assign_role_policy,
        roles::remove_role_policy,
        roles::list_user_roles,
        roles::assign_user_role,
        roles::remove_user_role,
        // Policies
        policies::list_policies,
        policies::create_policy,
        policies::get_policy,
        policies::update_policy,
        policies::delete_policy,
        policies::list_permissions,
        policies::create_permission,
        policies::update_permission,
        policies::delete_permission,
        policies::delete_collection_permissions,
        // App access
        apps::get_user_app_access,
        apps::set_user_app_access,
    ),
    info(
        title = "Alcedo Core API",
        description = "The Alcedo Core API documenation",
        contact(name = "Attacler",),
        license(
            name = "Custom, see Github for more details",
        ),
    ),
    tags(
        (name = "Items - Query", description = concat!(
            "<details><summary><strong>Filter reference</strong></summary>\n\n",
            include_str!("../../api-docs/query.md"),
            "\n\n</details>"
        )),
        (name = "Collections", description = "Collection and field definitions (app-scoped)"),
        (name = "Apps", description = "Applications and their version bindings"),
        (name = "Versions", description = "Deployment versions. `production` is the template version"),
        (name = "Developer Keys", description = "Version-scoped developer API keys"),
        (name = "Auth", description = "Session authentication"),
        (name = "Users", description = "User administration"),
        (name = "Sessions", description = "Session management"),
        (name = "Settings", description = "Platform settings"),
        (name = "Roles", description = "App-scoped roles, scopes and user assignments"),
        (name = "Policies", description = "Record access policies and permission rules"),
    ),
    components(
        schemas (
            DocsItemFilter,
            apps::AppWithVersions,
            apps::CreateAppRequest,
            apps::UpdateAppRequest,
            apps::UserAppAccess,
            versions::VersionRow,
            versions::CreateVersionRequest,
            developer_keys::DeveloperKeyResponse,
            developer_keys::CreateDeveloperKeyRequest,
            settings::SettingsResponse,
            auth::LoginRequest,
        )
    )
)]
struct ApiDoc;

pub fn docs_controller() -> Router<AppState> {
    return Router::new().route("/openapi.json", get(get_docs));
}

async fn get_docs() -> String {
    let docs = ApiDoc::openapi().to_json().unwrap().to_string();

    docs
}
