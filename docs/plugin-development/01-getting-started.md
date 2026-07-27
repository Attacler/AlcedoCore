# Getting Started with Alcedo Plugin Development

This guide walks you through creating your first Alcedo plugin — from scaffolding to a running plugin proxied by AlcedoCore.

## Prerequisites

- **Node.js 18+** (for the `alcedo` CLI)
- **Python 3.9+** (if developing Python plugins)
- **Docker** (for building and running plugins)
- A running **Alcedo-core** instance (see alcedocore README for setup)

## 1. Install the CLI

The `alcedo` CLI is located at `cli/alcedo/`. Install its dependencies:

```bash
cd cli/alcedo
npm install
npm run build
```

You can use it directly via `node dist/index.js` or alias it:

```bash
alias alcedo="node /path/to/alcedo/dist/index.js"
```

## 2. Scaffold a Plugin

Use `alcedo init` to generate a new plugin project:

```bash
alcedo init my-plugin
```

You will be prompted to choose a language — `python` or `node`:

```
Select plugin language (python/node): python
```

This creates:

```
my-plugin/
  manifest.json       # Plugin manifest (endpoints, pages, settings)
  server.py           # Python server stub
  Dockerfile          # Container build recipe
  .gitignore
  README.md
  migrations/         # Database migration SQL files
  pages/              # Vue 3 UI page components
  public/             # Static assets
```

### Language Templates

| Language | Server file | SDK package                    |
| -------- | ----------- | ------------------------------ |
| Python   | `server.py` | `alcedocore-sdk-python` (PyPI) |
| Node.js  | `server.js` | `alcedocore-sdk-node` (npm)    |

## 3. Understand the Manifest

The `manifest.json` is the plugin's registration document. At minimum it declares:

```json
{
    "name": "my-plugin",
    "version": "1.0.0",
    "plugin_type": "docker",
    "image": "localhost:5000/my-plugin:1.0.0",
    "env": {},
    "resources": {},
    "endpoints": [
        {
            "method": "GET",
            "path": "/api/hello",
            "description": "Greeting endpoint",
            "group": "api"
        }
    ]
}
```

See [05-manifest-schema.md](./05-manifest-schema.md) for the full schema reference.

## 4. Develop the Plugin Server

### Python

```python
from http.server import HTTPServer, BaseHTTPRequestHandler
import json

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/api/hello":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"message": "Hello from my-plugin!"}).encode())

if __name__ == "__main__":
    server = HTTPServer(("0.0.0.0", 8080), Handler)
    server.serve_forever()
```

### Node.js

```javascript
import http from "node:http";

const server = http.createServer((req, res) => {
    if (req.url === "/api/hello" && req.method === "GET") {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ message: "Hello from my-plugin!" }));
    }
});

server.listen(8080, () => console.log("Plugin listening on :8080"));
```

## 5. Build the Docker Image

```bash
cd my-plugin
docker build -t localhost:5000/my-plugin:1.0.0 .
```

## 6. Deploy via the Admin API

```bash
curl -X POST http://localhost:8080/admin/plugins/deploy \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "my-plugin",
    "version": "1.0.0",
    "image": "localhost:5000/my-plugin:1.0.0",
    "env": {}
  }'
```

## 7. Access the Plugin

Once deployed, all API endpoints declared in the manifest are proxied through:

```bash
curl http://localhost:8080/p/my-plugin/api/hello
```

## 8. Next Steps

- Use the SDK to access KV storage, database, and settings — see the language-specific SDK references:
    - [Python SDK Reference](./02-python-sdk.md)
    - [Node.js SDK Reference](./03-nodejs-sdk.md)
    - [Rust SDK Reference](./04-rust-sdk.md)
- Add database migrations — see [07-tutorial.md](./07-tutorial.md)
- Optimize your Dockerfile — see [08-best-practices.md](./08-best-practices.md)
