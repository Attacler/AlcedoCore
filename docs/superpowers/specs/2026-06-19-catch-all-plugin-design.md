# Catch-All Plugin — Design Spec

## Problem

When a request arrives that doesn't match any Rust route (e.g., the root path `/`),
the core returns a 404. For Nuxt-based plugins that want to serve content at the
root (`/`), there's no way to forward unmatched requests to a specific plugin.

## Solution

A system setting `catch_all_plugin_slug` selects which plugin receives all unmatched
requests. When empty (default), behavior is unchanged (404). When set to a plugin
slug, unmatched requests are proxied to that plugin's container.

## Architecture

Two intercept points in the request pipeline:

### 1. Axum `fallback` handler — `api/mod.rs`

Catches routes that match NO Axum route at all (primarily `/`).

- Reads `catch_all_plugin_slug` from the `system_settings` table
- If empty/missing, returns 404 as before
- If set, calls `proxy_handler` directly with the catch-all slug + original path

### 2. `serve_index_or_static` — `static_files.rs`

Catches requests matched by `/:slug/*path` where the slug isn't a known plugin.
Currently returns 404 at two points:

- `find_by_slug` returns `None` → no plugin with that slug exists
- `find_active` returns `None` → plugin exists but no deployed version

Both points now check the catch-all setting before returning 404.

### Proxy flow

For a request `/some-page` with `catch_all_plugin_slug = "hello-world-nuxt"`:

```
Request: GET /some-page
  ↓
/:slug/*path matches (slug="some-page", path="")
  ↓
serve_index_or_static: find_by_slug("some-page") → None
  ↓
Check catch_all_plugin_slug → "hello-world-nuxt"
  ↓
proxy_handler(slug="hello-world-nuxt", path="some-page")
  ↓
Target URL: http://<container_ip>:8080/some-page
  ↓
Nuxt plugin's server handles the request
```

## Files Changed

### `alcedocore/src/api/mod.rs`

- Add `catch_all_handler` as Axum `fallback` route (after all existing routes)
- Import `SystemSetting` from `crate::db::queries`
- Handler reads `catch_all_plugin_slug` from `system_settings` table
- Proxies to the configured plugin if set, otherwise returns 404

### `alcedocore/src/api/static_files.rs`

In `serve_index_or_static`:

- After `find_by_slug` returns `None` (no plugin with that slug): check catch-all
- After `find_active` returns `None` (plugin not deployed): check catch-all

Both paths: if catch-all is set, call `proxy_handler` with the catch-all slug
instead of the original slug, and the original path.

### `system-plugins/admin/src/stores/settingsStore.ts`

Add to `SETTING_META`:

```ts
catch_all_plugin_slug: {
    key: 'catch_all_plugin_slug',
    label: 'Catch-all Plugin',
    description: 'Plugin slug that receives all unmatched requests. Empty = disabled.',
    category: 'general',
    type: 'string',
    required: false,
    defaultValue: '',
},
```

This renders as a text input in the General section of the Settings page.

## Edge Cases

| Scenario                                             | Behavior                                                           |
| ---------------------------------------------------- | ------------------------------------------------------------------ |
| Setting is empty                                     | Normal 404 behavior, no regression                                 |
| Setting is set to non-existent slug                  | proxy_handler returns 404 (no active version)                      |
| Catch-all plugin is the one being requested normally | Normal flow — static files/proxy works as before                   |
| Setting changes between requests                     | Next request picks up new value from DB                            |
| High traffic on unmatched paths                      | DB read per request is acceptable (simple key lookup, small table) |

## Future Considerations

- Could cache `catch_all_plugin_slug` in AppState or Redis to avoid DB hits
- Could add a per-plugin indicator in the admin UI showing which plugin is the catch-all
- Could support multiple catch-all plugins with priority/weight

## Rejected Alternatives

- **Per-plugin toggle**: Only one plugin can be the catch-all, so a system setting
  is simpler and prevents conflicts.
- **Remove `/:slug/*path` route**: Too invasive, would break existing plugin routing.
- **Nginx-level redirect**: Would bypass the core's proxy logic and auth.
