use alcedo_common::context::RequestContext;
use axum::http::HeaderMap;
use std::sync::Arc;

use crate::api::permission_check;
use crate::db::queries::Plugin;
use crate::error::AppError;
use crate::plugins::health::AppState;

/// Resolve the install a slug-based endpoint should operate on.
///
/// Precedence:
/// 1. explicit `install_id` (management override)
/// 2. app install (`ctx.app_version_id`)
/// 3. version install (`ctx.version_id`)
/// 4. global install
/// 5. NotFound
///
/// This performs **no** authorization of the override — prefer
/// [`resolve_install_for_request_authorized`] from request handlers so an
/// `install_id` cannot be used to reach an install outside the caller's scope.
pub async fn resolve_install_for_context(
    db: &sqlx::PgPool,
    slug: &str,
    ctx: &RequestContext,
) -> Result<Plugin, AppError> {
    resolve_install_for_request(db, slug, ctx, None).await
}

/// Resolve the `(app_version_id, version_id)` scope for a slug under the
/// request context, using the install's own scope (most-specific-wins). Falls
/// back to the raw context scope when no install row exists so callers that
/// operate on plugin schemas directly still get a deterministic schema name.
pub async fn resolve_install_scope_for_context(
    db: &sqlx::PgPool,
    slug: &str,
    ctx: &RequestContext,
) -> Result<(Option<i32>, Option<i32>), AppError> {
    Ok(
        Plugin::resolve_install_scoped(db, slug, ctx.app_version_id, ctx.version_id)
            .await?
            .map(|p| (p.app_version_id, p.version_id))
            .unwrap_or((ctx.app_version_id, ctx.version_id)),
    )
}

pub async fn resolve_install_for_request(
    db: &sqlx::PgPool,
    slug: &str,
    ctx: &RequestContext,
    install_id: Option<i64>,
) -> Result<Plugin, AppError> {
    if let Some(id) = install_id {
        return Plugin::find_by_id(db, id)
            .await?
            .filter(|p| p.slug == slug)
            .ok_or_else(|| AppError::NotFound(format!("Plugin install not found: {}", id)));
    }

    Plugin::resolve_install_scoped(db, slug, ctx.app_version_id, ctx.version_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Plugin not found: {}", slug)))
}

/// Resolve an install for a request handler and authorize an explicit
/// `install_id` override against the caller's identity.
///
/// An override is accepted when:
/// - no override was given (context resolution is inherently scoped), or
/// - the install is reachable from the caller's context, or
/// - the caller is a global admin (`is_admin` or `users.all`), or
/// - the caller is a developer API key and the install belongs to the caller's
///   version (dev keys are version-scoped root credentials — the override must
///   not let them cross the version boundary).
///
/// Otherwise this returns `NotFound` so install existence outside the caller's
/// scope is not leaked.
pub async fn resolve_install_for_request_authorized(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    db: &sqlx::PgPool,
    slug: &str,
    ctx: &RequestContext,
    install_id: Option<i64>,
) -> Result<Plugin, AppError> {
    let install = resolve_install_for_request(db, slug, ctx, install_id).await?;

    if install_id.is_none() || install_reachable_from_context(&install, ctx) {
        return Ok(install);
    }

    if permission_check::is_global_admin_caller(state, headers).await? {
        return Ok(install);
    }

    if permission_check::is_valid_dev_key(headers)
        && install_within_context_version(db, &install, ctx).await?
    {
        return Ok(install);
    }

    Err(AppError::NotFound(format!("Plugin not found: {}", slug)))
}

/// True when the caller is privileged for the purpose of managing a broader
/// install: a global admin session (`is_admin`/`users.all`) or a validated
/// developer API key. This mirrors the privilege signal
/// [`resolve_install_for_request_authorized`] uses to accept an explicit
/// `install_id` override — no new signal is introduced.
pub async fn caller_is_privileged(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<bool, AppError> {
    if permission_check::is_valid_dev_key(headers) {
        return Ok(true);
    }
    permission_check::is_global_admin_caller(state, headers).await
}

/// Enforce Decision 8 of the three-level scoping spec: in an explicit app
/// context an install supplied by a broader scope (`version`/`global`) is
/// read-only and must be managed from the global zone.
///
/// A request is "in an app context" when the middleware resolved a concrete
/// `app_version_id` (i.e. an app was explicitly named via `X-App`/`?ac_app=`
/// **and** mapped to a real app×version). A named-but-unresolvable app or a
/// headerless request is version/global context, not app context, so it is not
/// restricted here.
///
/// The check is independent of `install_id`: supplying an explicit override
/// must not let a non-privileged caller mutate a broader install. Privileged
/// callers (global admin / validated developer API key) may still write
/// inherited installs — with or without `install_id` — so global-zone
/// management keeps working.
pub async fn ensure_install_writable(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    ctx: &RequestContext,
    install: &Plugin,
) -> Result<(), AppError> {
    if ctx.app_version_id.is_some()
        && install.scope() != "app"
        && !caller_is_privileged(state, headers).await?
    {
        return Err(AppError::Forbidden(
            "Inherited plugin installs are read-only in an app context; \
             manage this install from the global Plugins area."
                .to_string(),
        ));
    }
    Ok(())
}

/// True when the install is reachable under the caller's request context:
/// a global install, the app install matching `ctx.app_version_id`, or a
/// version install matching `ctx.version_id`.
fn install_reachable_from_context(install: &Plugin, ctx: &RequestContext) -> bool {
    if install.app_version_id.is_none() && install.version_id.is_none() {
        return true;
    }
    if let Some(app_version_id) = ctx.app_version_id {
        if install.app_version_id == Some(app_version_id) {
            return true;
        }
    }
    if let Some(version_id) = ctx.version_id {
        if install.app_version_id.is_none() && install.version_id == Some(version_id) {
            return true;
        }
    }
    false
}

/// True when the install belongs to the caller's resolved version. Used to
/// bound developer API keys, which are root for exactly one version. App
/// installs are resolved to their underlying version before comparison.
async fn install_within_context_version(
    db: &sqlx::PgPool,
    install: &Plugin,
    ctx: &RequestContext,
) -> Result<bool, AppError> {
    let Some(ctx_version_id) = ctx.version_id else {
        return Ok(false);
    };
    match (install.app_version_id, install.version_id) {
        (None, None) => Ok(true),
        (None, Some(version_id)) => Ok(version_id == ctx_version_id),
        (Some(app_version_id), _) => {
            let version_id: Option<i32> = sqlx::query_scalar(
                "SELECT version_id FROM alcedo.alcedo_apps_versions WHERE id = $1",
            )
            .bind(app_version_id)
            .fetch_optional(db)
            .await?;
            Ok(version_id == Some(ctx_version_id))
        }
    }
}
