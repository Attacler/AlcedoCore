# AlcedoCore Your backend build visually

AlcedoCore is a plugin-based backend platform that lets you build, deploy, and manage backend services through a visual admin interface. It orchestrates Docker containers and Kubernetes pods, and enforces fine-grained permission scopes. All configurable from the browser.

At its core sits **./core**, a Rust service that handles API routing, authentication, plugin lifecycle, proxy forwarding, and event-driven automation. The **Admin UI** (Vue/Vite) provides the visual layer for managing collections, items, users, roles, plugins, settings, and policies. The **Automation plugin** (JavaScript engine) reacts to item CRUD events with user-defined functions and triggers.

## Key Capabilities

- **Item Collections** Define collections with custom fields, sections, and views. The items API includes computed `$permissions` based on policy rules, controlling read/create/update/delete per user and per field.
- **KV Store** Key-value storage for plugins, with batch operations, TTL, and existence checks.
- **Automation Engine** JavaScript functions triggered by `ItemCreated`, `ItemUpdated`, `ItemDeleted` events. Functions run in a sandboxed JS runtime with access to the core API via granted scopes.
- **Event Forwarding** Forward events to external HTTP endpoints with configurable concurrency.
- **Dual Deployment** Runs on Docker Compose (Swarm mode) or on Kubernetes.
- **Plugin System** Two plugin types: _system plugins_ (admin, automation) bundled with the platform, and _user plugins_ (Docker images) deployed at runtime. Each plugin declares required scopes in a `manifest.json` and receives only the permissions it needs.

## Architecture

| Layer             | Technology                | Description                                                    |
| ----------------- | ------------------------- | -------------------------------------------------------------- |
| **core**          | Rust (Actix-web)          | API server, proxy, auth, plugin lifecycle, DB migrations       |
| **Admin UI**      | Vue 3 + Vite + PrimeVue   | Visual management of collections, items, users, roles, plugins |
| **Automation**    | JavaScript (Deno/Deno)    | Event-driven function execution on item CRUD                   |
| **Database**      | PostgreSQL                | Primary data store                                             |
| **Cache**         | Redis                     | Session store, X-Request-ID → slug mapping, rate limiting      |
| **Orchestration** | Docker Swarm / Kubernetes | Container lifecycle and networking                             |

## More Information

Visit [**alcedocore.nl**](https://alcedocore.nl) for documentation, pricing, and licensing.

## License

AlcedoCore is licensed under the **Business Source License 1.1**. See [LICENSE](./LICENSE) for full terms.

- **Free tier**: Use the Licensed Work if your Total Finances (revenue + funding) do not exceed €100,000 in the most recent 12-month period.
- **Commercial use**: Requires a paid license visit [alcedocore.nl/pricing](https://alcedocore.nl/pricing) for options.
- **Change Date**: Four years after first public distribution of a version, it converts to **GNU General Public License v3 or later (GPL-3.0-or-later)**.
