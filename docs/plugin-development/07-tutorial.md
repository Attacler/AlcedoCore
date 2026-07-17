# Tutorial: Building a Complete Plugin

This tutorial builds a **Task Manager** plugin with persistent storage, database migrations, a REST API, and a Vue 3 UI.

## What You'll Build

- ✅ Plugin scaffolded with `alcedo init`
- ✅ Key/Value storage for configuration
- ✅ Database table with migrations (up + down)
- ✅ REST API with CRUD endpoints
- ✅ Vue 3 UI pages
- ✅ Docker container

## Prerequisites

- Alcedo CLI installed (see [01-getting-started.md](./01-getting-started.md))
- Docker installed
- alcedocore running on `http://localhost:8080`

---

## Step 1: Scaffold the Plugin

```bash
alcedo init task-manager
# Select language: python
```

This creates:

```
task-manager/
  manifest.json
  server.py
  Dockerfile
  migrations/
  pages/
  public/
```

---

## Step 2: Add Navigation Items

Add a root page and a settings page:

```bash
alcedo add page tasks --route /tasks --icon list
alcedo add page settings --route /settings --icon settings
```

This updates `manifest.json` and creates `pages/tasks.vue` and `pages/settings.vue`.

---

## Step 3: Add API Endpoints

```bash
alcedo add endpoint get_tasks --method GET --path /api/tasks
alcedo add endpoint create_task --method POST --path /api/tasks
alcedo add endpoint delete_task --method DELETE --path /api/tasks/:id
```

This creates handler stubs in `endpoints/` and updates `manifest.json`.

---

## Step 4: Create a Database Migration

```bash
alcedo add migration create_tasks_table
```

This generates:

```
migrations/20260527120000_create_tasks_table.up.sql
migrations/20260527120000_create_tasks_table.down.sql
```

Edit the **up migration** (`...up.sql`):

```sql
CREATE TABLE tasks (
    id SERIAL PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    description TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
```

Edit the **down migration** (`...down.sql`):

```sql
DROP TABLE IF EXISTS tasks;
```

---

## Step 5: Write the Plugin Server

Edit `server.py` to serve the API and use the SDK:

```python
import os
import json
import asyncio
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse

from alcedo_sdk import AlcedoClient

CORE_URL = os.environ.get("CORE_URL", "http://localhost:8080")
PLUGIN_SLUG = "task-manager"
_loop = asyncio.new_event_loop()
asyncio.set_event_loop(_loop)

client = AlcedoClient(base_url=CORE_URL, plugin_slug=PLUGIN_SLUG)

def _run(coro):
    return _loop.run_until_complete(coro)


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path

        if path == "/health":
            return self._json({"status": "healthy"})

        elif path == "/api/tasks":
            # List tasks via SDK database query
            result = _run(client.db.query(
                "SELECT id, title, description, status, created_at FROM tasks ORDER BY id ASC"
            ))
            tasks = []
            for row in result["rows"]:
                tasks.append({
                    "id": row[0],
                    "title": row[1],
                    "description": row[2],
                    "status": row[3],
                    "created_at": row[4],
                })
            return self._json({"tasks": tasks})

        else:
            self._json({"error": "not_found"}, 404)

    def do_POST(self):
        parsed = urlparse(self.path)
        path = parsed.path

        if path == "/api/tasks":
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length)) if length else {}

            title = body.get("title", "").strip()
            if not title:
                return self._json({"error": "title is required"}, 400)

            result = _run(client.db.query(
                "INSERT INTO tasks (title, description, status) VALUES ($1, $2, $3) RETURNING id",
                params=[title, body.get("description", ""), "pending"],
            ))
            task_id = result["rows"][0][0] if result["rows"] else None
            return self._json({"id": task_id, "status": "created"}, 201)

        else:
            self._json({"error": "not_found"}, 404)

    def do_DELETE(self):
        parsed = urlparse(self.path)
        path = parsed.path

        if path.startswith("/api/tasks/"):
            task_id = path.split("/")[-1]
            _run(client.db.query("DELETE FROM tasks WHERE id = $1", params=[int(task_id)]))
            return self._json({"status": "deleted"})
        else:
            self._json({"error": "not_found"}, 404)

    def _json(self, data, status=200):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())


if __name__ == "__main__":
    # Initialize the SDK client
    _run(client.__aenter__())

    server = HTTPServer(("0.0.0.0", 8080), Handler)
    print(f"Task Manager plugin starting on :8080")
    print(f"Core URL: {CORE_URL}")
    server.serve_forever()
```

---

## Step 6: Verify the Manifest

Edit `manifest.json` to ensure endpoints are declared:

```json
{
    "name": "task-manager",
    "version": "1.0.0",
    "plugin_type": "docker",
    "image": "localhost:5000/task-manager:1.0.0",
    "env": {},
    "endpoints": [
        {
            "method": "GET",
            "path": "/api/tasks",
            "description": "List all tasks",
            "group": "api"
        },
        {
            "method": "POST",
            "path": "/api/tasks",
            "description": "Create a task",
            "group": "api"
        },
        {
            "method": "DELETE",
            "path": "/api/tasks/:id",
            "description": "Delete a task",
            "group": "api"
        }
    ],
    "pages": [
        { "label": "Tasks", "path": "/tasks", "sidebar": true, "icon": "list" },
        {
            "label": "Settings",
            "path": "/settings",
            "sidebar": true,
            "icon": "settings"
        }
    ],
    "settings_schema": {
        "type": "object",
        "properties": {
            "page_size": {
                "type": "integer",
                "title": "Page Size",
                "default": 20,
                "description": "Number of tasks per page"
            }
        }
    }
}
```

---

## Step 7: Build the Docker Image

```dockerfile
FROM python:3.11-slim

WORKDIR /app

# Install SDK
COPY sdk/python/ ./sdk/
RUN pip install ./sdk/

# Copy plugin code
COPY task-manager/ .

EXPOSE 8080
CMD ["python", "server.py"]
```

```bash
docker build -t localhost:5000/task-manager:1.0.0 .
```

---

## Step 8: Deploy and Run Migrations

Deploy the plugin:

```bash
curl -X POST http://localhost:8080/admin/plugins/deploy \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "task-manager",
    "version": "1.0.0",
    "image": "localhost:5000/task-manager:1.0.0",
    "env": {}
  }'
```

Run migrations using the `alcedo migrate` CLI:

```bash
# Navigate to the plugin directory
cd task-manager

# Upload migration files to alcedocore
alcedo migrate run
```

Or upload and run via the SDK:

```python
# manually via Python
result = await client.migrations.run()
print(result)
```

---

## Step 9: Test the API

```bash
# Create a task
curl -X POST http://localhost:8080/p/task-manager/api/tasks \
  -H "Content-Type: application/json" \
  -d '{"title": "Write documentation", "description": "Document the plugin API"}'

# List tasks
curl http://localhost:8080/p/task-manager/api/tasks

# Delete a task
curl -X DELETE http://localhost:8080/p/task-manager/api/tasks/1
```

Expected output:

```json
// POST /api/tasks
{ "id": 1, "status": "created" }

// GET /api/tasks
{
  "tasks": [
    {
      "id": 1,
      "title": "Write documentation",
      "description": "Document the plugin API",
      "status": "pending",
      "created_at": "2026-05-27T12:00:00Z"
    }
  ]
}
```

---

## Step 10: Write a Vue 3 Page

Edit `pages/tasks.vue`:

```vue
<template>
    <div class="tasks-page">
        <h1>Tasks</h1>

        <div class="create-form">
            <input
                v-model="newTask"
                placeholder="New task title"
                @keyup.enter="createTask"
            />
            <button @click="createTask">Add</button>
        </div>

        <ul class="task-list">
            <li v-for="task in tasks" :key="task.id">
                <span :class="{ done: task.status === 'done' }">{{
                    task.title
                }}</span>
                <button @click="deleteTask(task.id)">✕</button>
            </li>
        </ul>
    </div>
</template>

<script setup>
import { ref, onMounted } from "vue";

const tasks = ref([]);
const newTask = ref("");

async function fetchTasks() {
    const res = await fetch("/api/tasks");
    const data = await res.json();
    tasks.value = data.tasks || [];
}

async function createTask() {
    if (!newTask.value.trim()) return;
    await fetch("/api/tasks", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ title: newTask.value }),
    });
    newTask.value = "";
    await fetchTasks();
}

async function deleteTask(id) {
    await fetch(`/api/tasks/${id}`, { method: "DELETE" });
    await fetchTasks();
}

onMounted(fetchTasks);
</script>

<style scoped>
/* Add your styles here */
</style>
```

---

## Step 11: Local Development with Hot Reload

The `alcedo dev` command provides local development without rebuilding the Docker image each time:

```bash
cd task-manager
alcedo dev --port 3000
```

This:

1. Registers a dev session with alcedocore (routes `/p/task-manager/*` to `http://localhost:3000`)
2. Starts your `server.py` or `server.js` locally
3. Watches for file changes and auto-restarts the server
4. Cleans up the dev session on exit

```bash
# Access your plugin through the proxy
curl http://localhost:8080/p/task-manager/api/tasks
```

---

## Summary

You've built a complete plugin with:

| Feature             | How                                           |
| ------------------- | --------------------------------------------- |
| Scaffolding         | `alcedo init`                                 |
| Database migrations | `alcedo add migration` + `alcedo migrate run` |
| API endpoints       | Manual in `server.py`                         |
| SDK usage           | `AlcedoClient.db.query()`                     |
| UI pages            | Vue 3 SFC in `pages/`                         |
| Containerization    | `Dockerfile`                                  |
| Local development   | `alcedo dev`                                  |
| Deployment          | Admin API deploy endpoint                     |

### Next Steps

- Add more SDK features (KV storage, settings, logs) — see [02-python-sdk.md](./02-python-sdk.md)
- Optimize your Dockerfile — see [08-best-practices.md](./08-best-practices.md)
- Add TTL-based key expiry for temporary data
- Implement settings UI with `settings_schema` in manifest
