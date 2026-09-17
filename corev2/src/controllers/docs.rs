use axum::{Router, routing::get};
use utoipa::OpenApi;

use crate::AppState;
use crate::controllers::items::DocsItemFilter;
use crate::controllers::{apps, collections, items};

#[derive(OpenApi)]
#[openapi(
    paths(
        collections::get_schema,
        collections::get_ts_schema,
        collections::get_collections,
        collections::create_collection,
        collections::drop_collection,
        collections::add_field,
        collections::drop_field,
        collections::update_field,
        apps::get_apps,
        items::get_items,
        items::create_items,
        items::update_items,
        items::delete_items,
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
        (name = "Items - Query", description = include_str!("../../api-docs/query.md"))
    ),
    components(
        schemas (
            DocsItemFilter
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
