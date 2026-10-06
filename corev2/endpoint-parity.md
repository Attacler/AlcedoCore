# corev2 endpoint parity vs v1 `../core`

Comparison of every route exposed by v1 (`core/alcedo-api/src/api`) against corev2.
corev2's implemented set is taken from the live OpenAPI spec
(`GET /api/docs/openapi.json`, 64 path templates), which is the authoritative list
of registered routes.

v1 endpoints are listed with their v1 paths. corev2 remounts the same handlers under
`/api/app` (app-scoped) and `/api/platform` (platform-scoped); equivalent routes are counted
as implemented regardless of the new prefix.

This is a review of **route surface**, not exact semantics. Notable intentional differences:
schema handling (`/api/app/collections/schema`, `schema/ts` are new), settings split into
platform + app scopes, developer keys moved to `/api/platform/developer-keys`, and items
update uses `PATCH` rather than v1's `PUT`.

## Implemented

| endpoint | short description | implemented (yes/no) |
| --- | --- | --- |
| POST /api/auth/login | Session login | yes |
| POST /api/auth/logout | Session logout | yes |
| GET /api/auth/me | Current user profile | yes |
| GET /api/users | List users | yes |
| POST /api/users | Create user | yes |
| GET /api/users/:id | Get user | yes |
| PUT /api/users/:id | Update user | yes |
| DELETE /api/users/:id | Delete user | yes |
| POST /api/users/:id/password | Change user password | yes |
| GET /api/users/:id/roles | List user roles | yes |
| POST /api/users/:id/roles | Assign role to user | yes |
| DELETE /api/users/:id/roles/:role_id | Remove role from user | yes |
| GET /api/users/:id/app-access | Get user app access | yes |
| PUT /api/users/:id/app-access | Set user app access | yes |
| GET /api/apps | List apps | yes |
| POST /api/apps | Create app | yes |
| GET /api/apps/:id | Get app | yes |
| PUT /api/apps/:id | Update app | yes |
| DELETE /api/apps/:id | Delete app | yes |
| GET /api/me/apps | Apps accessible to caller | yes |
| GET /api/versions | List versions | yes |
| POST /api/versions | Create version | yes |
| DELETE /api/versions/:id | Delete version | yes |
| GET /api/versions/:id/keys | List developer keys for version | yes |
| GET /api/settings | Get all settings | yes |
| PUT /api/settings/:key | Update settings key | yes |
| POST /api/settings/batch | Batch update settings | yes |
| GET /api/settings/developer/keys | List developer keys | yes |
| POST /api/settings/developer/keys | Create developer key | yes |
| DELETE /api/settings/developer/keys/:id | Delete developer key | yes |
| GET /api/roles | List roles | yes |
| POST /api/roles | Create role | yes |
| GET /api/roles/:id | Get role | yes |
| PUT /api/roles/:id | Update role | yes |
| DELETE /api/roles/:id | Delete role | yes |
| GET /api/roles/:id/permissions | List role scopes | yes |
| POST /api/roles/:id/permissions | Set role scopes | yes |
| DELETE /api/roles/:id/permissions/:permission_id | Remove role scope | yes |
| GET /api/roles/:id/policies | List role policies | yes |
| POST /api/roles/:id/policies | Assign policy to role | yes |
| DELETE /api/roles/:id/policies/:policy_id | Remove policy from role | yes |
| GET /api/policies | List policies | yes |
| POST /api/policies | Create policy | yes |
| GET /api/policies/:id | Get policy | yes |
| PUT /api/policies/:id | Update policy | yes |
| DELETE /api/policies/:id | Delete policy | yes |
| GET /api/policies/:id/permissions | List policy permissions | yes |
| POST /api/policies/:id/permissions | Create policy permission | yes |
| PUT /api/policies/:id/permissions/:pid | Update permission | yes |
| DELETE /api/policies/:id/permissions/:pid | Delete permission | yes |
| DELETE /api/policies/:id/permissions/collection/:name | Delete collection permissions | yes |
| GET /api/menus/my | Current user menus | yes |
| GET /api/menus | List menus | yes |
| POST /api/menus | Create menu | yes |
| GET /api/menus/:id | Get menu | yes |
| PUT /api/menus/:id | Update menu | yes |
| DELETE /api/menus/:id | Delete menu | yes |
| GET /api/menus/:id/roles | Get menu role grants | yes |
| PUT /api/menus/:id/roles | Set menu role grants | yes |
| POST /api/menus/:id/copy | Copy menu tree | yes |
| GET /api/collections | List collections | yes |
| POST /api/collections | Create collection | yes |
| GET /api/collections/:name | Get collection | yes |
| PUT /api/collections/:name | Update collection | yes |
| DELETE /api/collections/:name | Delete collection | yes |
| GET /api/collections/:name/$create | Create-permission check | yes |
| GET /api/collections/:name/layouts | List layouts | yes |
| POST /api/collections/:name/layouts | Create layout | yes |
| GET /api/collections/:name/layout | Resolve layout | yes |
| PUT /api/collections/:name/layouts/:layout_id | Update layout | yes |
| DELETE /api/collections/:name/layouts/:layout_id | Delete layout | yes |
| GET /api/collections/:name/layouts/:layout_id/roles | Get layout roles | yes |
| PUT /api/collections/:name/layouts/:layout_id/roles | Set layout roles | yes |
| GET /api/collections/:name/layouts/:layout_id/sections | List sections | yes |
| POST /api/collections/:name/layouts/:layout_id/sections | Create section | yes |
| PATCH /api/collections/:name/layouts/:layout_id/sections | Reorder sections | yes |
| PUT /api/collections/:name/layouts/:layout_id/sections/:section_id | Update section | yes |
| DELETE /api/collections/:name/layouts/:layout_id/sections/:section_id | Delete section | yes |
| GET /api/items/:slug | List items | yes |
| POST /api/items/:slug | Create items | yes |
| PUT /api/items/:slug | Update items by filter | yes |
| DELETE /api/items/:slug | Delete items | yes |
| GET /api/items/:slug/:id | Get item | yes |
| PATCH /api/items/:slug/:id | Update single item | yes |
| GET /api/items/:slug/:id/references | Get item references | yes |
| POST /api/files/upload | Upload file | yes |
| POST /api/files/batch/delete | Batch delete files | yes |
| GET /api/files | List files | yes |
| GET /api/files/:id | Get file metadata | yes |
| PATCH /api/files/:id | Update file metadata | yes |
| DELETE /api/files/:id | Delete file | yes |
| GET /api/files/:id/download | Download file | yes |
| GET /api/files/folders | List folders | yes |
| POST /api/files/folders | Create folder | yes |
| GET /api/files/folders/:id | Get folder | yes |
| PATCH /api/files/folders/:id | Update folder | yes |
| DELETE /api/files/folders/:id | Delete folder | yes |
| GET /api/kv | List KV keys | yes |
| GET /api/kv/:key | Get KV value | yes |
| PUT /api/kv/:key | Set KV value | yes |
| DELETE /api/kv/:key | Delete KV entry | yes |
| GET /api/kv/:key/exists | Check KV key exists | yes |
| GET /api/kv/:key/ttl | Get KV key TTL | yes |
| POST /api/kv/:key/increment | Increment KV counter | yes |
| POST /api/kv/:key/decrement | Decrement KV counter | yes |
| POST /api/kv/batch/get | Batch get KV values | yes |
| POST /api/kv/batch/set | Batch set KV values | yes |
| POST /api/kv/batch/delete | Batch delete KV values | yes |
| POST /api/dev/request-id | Mint plugin auth request ID | yes |

## Not implemented

| endpoint | short description | implemented (yes/no) |
| --- | --- | --- |
| GET /api/apps/:app_id/versions/:version_id/access | List app-version access grants | no |
| PUT /api/apps/:app_id/versions/:version_id/access | Set app-version access grants | no |
| DELETE /api/apps/:app_id/versions/:version_id/access/:user_id | Revoke user app-version access | no |
| GET /api/policies/:id/plugins | List plugins assigned to policy | no |
| GET /api/plugins/:slug/policies | List policies for plugin | no |
| POST /api/plugins/:slug/policies | Assign policy to plugin | no |
| DELETE /api/plugins/:slug/policies/:policyId | Remove policy from plugin | no |
| GET /api/collections/:name/views | List saved views | no |
| POST /api/collections/:name/views | Create saved view | no |
| PUT /api/collections/:name/views/:id | Update saved view | no |
| DELETE /api/collections/:name/views/:id | Delete saved view | no |
| PUT /api/collections/:name/views/:id/default | Set default saved view | no |
| POST /api/items/:slug/query | Query items | no |
| POST /api/items/:slug/grouped | Grouped item counts | no |
| GET /api/logs/system | List system logs | no |
| GET /api/logs/collections | List collection logs | no |
| GET /api/plugins | List plugins | no |
| POST /api/plugins | Create plugin | no |
| POST /api/plugins/preview | Preview plugin manifest | no |
| GET /api/plugins/:slug | Get plugin | no |
| PUT /api/plugins/:slug | Update plugin | no |
| DELETE /api/plugins/:slug | Delete plugin | no |
| POST /api/plugins/:slug/enable | Enable plugin | no |
| POST /api/plugins/:slug/disable | Disable plugin | no |
| POST /api/plugins/:slug/deploy | Deploy plugin | no |
| GET /api/plugins/:slug/versions | List plugin versions | no |
| GET /api/plugins/:slug/instances/:instanceId/logs | Get instance logs | no |
| POST /api/plugins/deploy | Deploy plugin (admin) | no |
| POST /api/plugins/:slug/stop | Stop plugin | no |
| POST /api/plugins/:slug/restart | Restart plugin | no |
| POST /api/plugins/:slug/scale | Scale plugin replicas | no |
| GET /api/plugins/:slug/instances | List plugin instances | no |
| GET /api/plugins/:slug/instances/:instanceId | Get plugin instance | no |
| GET /api/plugins/:slug/instances/:instanceId/stats | Get instance stats | no |
| GET /api/plugins/:slug/events | Get event subscriptions | no |
| GET /api/plugins/:slug/schema | Get plugin schema | no |
| GET /api/plugins/:slug/migrations | List plugin migrations | no |
| POST /api/plugins/:slug/migrations | Run plugin migration | no |
| POST /api/plugins/:slug/migrations/upload | Upload plugin migrations | no |
| POST /api/plugins/:slug/rollback/:version | Rollback plugin migration | no |
| GET /api/plugins/:slug/settings | Get plugin settings | no |
| PATCH /api/plugins/:slug/settings | Update plugin settings | no |
| GET /api/plugins/:slug/pages | Get plugin pages | no |
| GET /api/plugins/:slug/pages/assets | Get plugin page assets | no |
| GET /api/plugins/:slug/runtime | Get plugin runtime info | no |
| GET /api/plugins/:slug/logs | Get plugin logs | no |
| GET /api/plugins/:slug/logs/:requestId | Get request log detail | no |
| GET /api/plugins/:slug/scopes | Get plugin scopes | no |
| POST /api/plugins/:slug/scopes | Update plugin scopes | no |
| GET /api/plugins/:slug/docs | List plugin docs | no |
| GET /api/plugins/:slug/docs/*path | Fetch plugin doc file | no |
| GET /api/registries | List registries | no |
| POST /api/registries | Create registry | no |
| POST /api/registries/health-check | Health check registry URL | no |
| GET /api/registries/:id | Get registry | no |
| PUT /api/registries/:id | Update registry | no |
| DELETE /api/registries/:id | Delete registry | no |
| GET /api/registries/:id/health | Health check registry | no |
| GET /api/registries/:id/images | List registry images | no |
| GET /api/openapi.json | OpenAPI spec | no |
| GET /health | Health check | no |
| ANY /p/:slug | Proxy request to plugin | no |
| ANY /p/:slug/*path | Proxy request to plugin path | no |
| GET /:slug/public/*path | Serve plugin static files | no |
| GET /:slug | Serve plugin index or static | no |
| GET /:slug/*path | Serve plugin nested static | no |
| ANY /api/internal-registry-proxy/*path | Registry image proxy | no |

## Summary

- 109 of 176 v1 endpoints implemented.
- Missing: 67 — all of Plugins, Registries, Logs, Saved Views,
  item `query`/`grouped`, plugin↔policy assignment, app×version access grants,
  `/health`, and the proxy/static/registry-proxy routes.
  (KV is **implemented** as app-scoped `/api/app/kv` backed by the cache;
  files are **implemented**.)
