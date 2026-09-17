use alcedo_common::context::{
    context_params_from_query, headers_to_app_context, RequestContext,
};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

use crate::plugins::health::AppState;

/// Attach a `RequestContext` extension to every request.
///
/// Resolution:
/// - `X-App`/`X-Version` headers default the version to `production`; the app
///   dimension is only used when explicitly provided. These headers drive
///   `/api/*` requests.
/// - `?ac_app=`/`?ac_version=` query params override the headers, but only for
///   non-API (asset) paths. `/api/*` requests ignore them so context stays
///   consistent with the auth middleware and `db_for_headers`.
/// - `version_id` is resolved from the version name; a plugin callback
///   (`X-Request-ID`) adopts its install's `version_id` (including `None`).
/// - `app_version_id` is resolved only when the app is explicit; a plugin
///   callback (`X-Request-ID`) adopts its install's `app_version_id`
///   (plugin callbacks operate strictly in their install's context, so they
///   cannot inherit a forwarded `X-App`). `app_explicit` still reflects only
///   whether the request itself named an app.
pub async fn context_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let headers = request.headers().clone();
    let mut app_ctx = headers_to_app_context(&headers);
    let mut app_explicit = headers.contains_key("x-app");

    // Query params (`?ac_app=` / `?ac_version=`) win over headers, but only for
    // asset paths — they must not alter context for `/api/*` requests.
    let is_api = request.uri().path().starts_with("/api/");
    let (q_app, q_version) = if is_api {
        (None, None)
    } else {
        context_params_from_query(request.uri().query())
    };
    if let Some(app) = q_app {
        app_ctx.app_name = app;
        app_explicit = true;
    }
    if let Some(version) = q_version {
        app_ctx.version = version;
    }

    let mut app_version_id = if app_explicit {
        state.resolve_app_version_id(&app_ctx).await.ok().flatten()
    } else {
        None
    };
    let mut version_id = state.resolve_version_id(&app_ctx.version).await.ok().flatten();

    // Plugin callbacks: the calling install's app version and version override
    // the headers, so callbacks operate in their install's context.
    if let Some(rid) = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(inst) = crate::proxy::lookup_install_by_request_id(&state.redis, rid).await {
            app_version_id = inst.app_version_id;
            version_id = inst.version_id;
        }
    }

    request
        .extensions_mut()
        .insert(RequestContext {
            app_context: app_ctx,
            app_version_id,
            version_id,
            app_explicit,
        });
    next.run(request).await
}
