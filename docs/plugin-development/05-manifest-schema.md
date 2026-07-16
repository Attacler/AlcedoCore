# Plugin Manifest Schema Reference

Every Alcedo plugin must have a `manifest.json` file at its root. This document declares the plugin's metadata, endpoints, UI pages, settings, and resource requirements to plugin-core.

## Example (hello-world plugin)

```json
{
  "name": "hello-world",
  "version": "2.2.1",
  "plugin_type": "docker",
  "system_plugin": true,
  "image": "local/static",
  "env": {},
  "resources": {},
  "pages": [
    { "label": "Hello", "path": "/", "sidebar": true },
    { "label": "Counter", "path": "/counter", "sidebar": true },
    { "label": "DB Items", "path": "/items", "sidebar": true }
  ],
  "endpoints": [
    {
      "method": "GET",
      "path": "/api/hello",
      "description": "Returns greeting message",
      "group": "api"
    },
    {
      "method": "GET",
      "path": "/api/counter",
      "description": "Persistent counter via AlcedoKV SDK",
      "group": "api"
    }
  ],
  "settings_schema": {
    "type": "object",
    "properties": {
      "greeting": {
        "type": "string",
        "title": "Greeting Message",
        "default": "Hello!",
        "description": "Custom greeting message"
      }
    }
  }
}
```

---

## Top-Level Fields

| Field             | Type     | Required | Description                                            |
| ----------------- | -------- | -------- | ------------------------------------------------------ |
| `name`            | string   | ✅       | Plugin name/slug (used for routing, e.g., `my-plugin`) |
| `version`         | string   | ✅       | Semantic version string (e.g., `1.0.0`)                |
| `plugin_type`     | string   | ✅       | Type of plugin — currently only `"docker"`             |
| `system_plugin`   | boolean  | —        | Whether this is a system-level plugin. Default: `false` |
| `image`           | string   | ✅       | Docker image reference (e.g., `localhost:5000/my-plugin:1.0.0`) |
| `env`             | object   | —        | Environment variables to inject into the container     |
| `resources`       | object   | —        | Resource quotas/limits                                 |
| `endpoints`       | array    | —        | API endpoint declarations                              |
| `pages`           | array    | —        | Vue 3 UI page declarations (displayed in admin UI)     |
| `settings_schema` | object   | —        | JSON Schema for plugin settings UI                     |

---

## `name`

The plugin slug. Must be unique across all plugins registered with a plugin-core instance. Used in:

- Proxy routing: `http://localhost:8080/p/{name}/...`
- API endpoints: `/api/plugins/{name}/...`

Valid characters: lowercase alphanumeric and hyphens.

## `version`

Semantic version string. Follows `MAJOR.MINOR.PATCH` conventions. Used for plugin lifecycle tracking and updates.

## `plugin_type`

Currently only `"docker"` is supported. This tells plugin-core to run the plugin as a Docker container using the specified `image`.

## `system_plugin`

If `true`, the plugin is considered a system-level plugin and is managed differently by plugin-core (e.g., auto-started, not user-removable).

## `image`

Docker image reference. Examples:

- `localhost:5000/my-plugin:1.0.0` — local registry
- `my-registry.example.com/plugins/my-plugin:latest` — remote registry
- `local/static` — loaded directly from the `.docker-plugins/` directory (for development/bundled plugins)

## `env`

Key-value map of environment variables injected into the plugin container at runtime.

```json
{
  "env": {
    "LOG_LEVEL": "debug",
    "MAX_CONNECTIONS": "100"
  }
}
```

Note: `CORE_URL` and `PORT` are injected automatically by plugin-core — do not set them manually.

---

## `endpoints` Array

Declares the HTTP endpoints the plugin serves. Each endpoint:

```json
{
  "method": "GET",
  "path": "/api/items",
  "description": "List all items",
  "group": "api"
}
```

### Endpoint Fields

| Field         | Type   | Required | Description                                      |
| ------------- | ------ | -------- | ------------------------------------------------ |
| `method`      | string | ✅       | HTTP method: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS` |
| `path`        | string | ✅       | URL path served by the plugin (e.g., `/api/hello`) |
| `description` | string | —        | Human-readable description of the endpoint       |
| `group`       | string | —        | Logical grouping (e.g., `"api"`, `"system"`, `"admin"`) |

Plugin-core uses these declarations for:

- Proxy routing setup
- Permission/access control
- API documentation generation
- Request logging metadata

---

## `pages` Array

Declares Vue 3 UI pages for the plugin. These appear in the admin UI sidebar.

```json
{
  "label": "Dashboard",
  "path": "/dashboard",
  "icon": "dashboard",
  "sidebar": true
}
```

### Page Fields

| Field     | Type    | Required | Description                                               |
| --------- | ------- | -------- | --------------------------------------------------------- |
| `label`   | string  | ✅       | Display name shown in sidebar and page header             |
| `path`    | string  | ✅       | Client-side route path (e.g., `"/counter"`)               |
| `icon`    | string  | —        | Material Symbols icon name (e.g., `"dashboard"`, `"settings"`) |
| `sidebar` | boolean | —        | Whether to show in sidebar navigation. Default: `false`    |
| `parent`  | string  | —        | Parent nav-item path for nesting (creates submenus)       |
| `badge`   | string  | —        | Badge text displayed next to the nav item                  |

### Page Path Conventions

- `"/"` — Root page (landing page for the plugin)
- `"/counter"`, `"/items"` — Named pages
- Paths correspond to Vue Router routes in the plugin's `pages/` directory

The `icon` field accepts any [Material Symbols icon name](https://fonts.google.com/icons).

---

## `settings_schema`

JSON Schema (draft-07) for the plugin's settings. plugin-core uses this to auto-generate a settings UI in the admin interface.

```json
{
  "settings_schema": {
    "type": "object",
    "properties": {
      "greeting": {
        "type": "string",
        "title": "Greeting Message",
        "default": "Hello!",
        "description": "Custom greeting message displayed on the plugin page"
      },
      "refresh_interval": {
        "type": "integer",
        "title": "Refresh Interval",
        "default": 30,
        "description": "Auto-refresh interval in seconds"
      },
      "debug_mode": {
        "type": "boolean",
        "title": "Debug Mode",
        "default": false,
        "description": "Enable verbose debug logging"
      },
      "max_items": {
        "type": "integer",
        "title": "Max Items",
        "minimum": 1,
        "maximum": 1000,
        "default": 100
      }
    },
    "required": []
  }
}
```

### Supported Types

| JSON Schema Type | UI Control     |
| ---------------- | -------------- |
| `string`         | Text input     |
| `integer`        | Number input   |
| `number`         | Number input   |
| `boolean`        | Toggle/switch  |
| `array`          | Multi-select   |

### Settings Access

Settings are accessed at runtime via the SDK:

```python
# Python
settings = await client.settings.get()
greeting = settings["settings"]["greeting"]
```

```typescript
// Node.js
const settings = await client.settings.get("my-plugin");
```

---

## Resources (`resources`)

Reserved for future resource quota declarations (CPU, memory, network). Currently unused — pass an empty object `{}`.

---

## Full Zod Schema (Node.js SDK)

The Node.js SDK exports a `PluginManifestSchema` for validating manifests at build time:

```typescript
import { PluginManifestSchema } from "alcedo-sdk";

const manifest = PluginManifestSchema.parse(rawManifest);
// {
//   manifestVersion: number,
//   pluginSlug: string,
//   pages?: PluginPage[],
//   views?: PluginView[],
//   inputWidgets?: PluginInputWidget[],
//   displayComponents?: PluginDisplayComponent[],
//   viewTypes?: PluginViewType[],
// }
```
