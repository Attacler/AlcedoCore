import express from "express";
import { createClient, NotFoundError } from "alcedo-sdk-node";

const PORT = parseInt(process.env.PORT || "8080", 10);
const CORE_URL = process.env.CORE_URL || "http://localhost:8080";
const PLUGIN_SLUG = process.env.PLUGIN_SLUG || "hello-world-node";

// ─── SDK Client ────────────────────────────────────────────────────────────
const client = createClient(CORE_URL, {
    timeout: 30_000,
    retry: { limit: 2 },
});

const app = express();
app.use(express.json());

// ─── Middleware: request logging ───────────────────────────────────────────
app.use((req, res, next) => {
    const start = Date.now();
    res.on("finish", () => {
        const ms = Date.now() - start;
        console.log(
            `[${new Date().toISOString()}] ${req.method} ${req.path} → ${res.statusCode} (${ms}ms)`,
        );
    });
    next();
});

// ─── Task 1: Health Check ──────────────────────────────────────────────────
app.get("/health", async (_req, res) => {
    try {
        const health = await client.health.check();
        res.json({
            status: "healthy",
            service: "hello-world-node",
            sdk: health,
        });
    } catch (err) {
        res.json({
            status: "degraded",
            service: "hello-world-node",
            error: err.message,
        });
    }
});

// ─── Task 2: Basic Greeting ────────────────────────────────────────────────
app.get("/api/hello", async (_req, res) => {
    res.json({
        status: "ok",
        message: "Hello from Node.js plugin!",
        path: "/api/hello",
        language: "Node.js",
        sdk_version: "0.2.0",
    });
});

// ─── Task 3: KV CRUD ──────────────────────────────────────────────────────

// GET /api/kv/:key — Retrieve a KV entry
app.get("/api/kv/:key", async (req, res) => {
    try {
        const value = await client.kv.get(req.params.key);
        res.json({ key: req.params.key, value });
    } catch (err) {
        if (err instanceof NotFoundError) {
            res.status(404).json({
                key: req.params.key,
                value: null,
                error: "not_found",
            });
        } else {
            res.status(500).json({ key: req.params.key, error: err.message });
        }
    }
});

// PUT /api/kv/:key — Create or update a KV entry
app.put("/api/kv/:key", async (req, res) => {
    try {
        const { value, ttl } = req.body;
        const result = await client.kv.set(req.params.key, value ?? "", ttl);
        res.json({ key: req.params.key, value: value ?? "", ttl, result });
    } catch (err) {
        res.status(500).json({ key: req.params.key, error: err.message });
    }
});

// DELETE /api/kv/:key — Delete a KV entry
app.delete("/api/kv/:key", async (req, res) => {
    try {
        const result = await client.kv.delete(req.params.key);
        res.json({ key: req.params.key, deleted: true, result });
    } catch (err) {
        res.status(500).json({ key: req.params.key, error: err.message });
    }
});

// GET /api/kv/:key/ttl — Check TTL of a KV entry
app.get("/api/kv/:key/ttl", async (req, res) => {
    try {
        const ttl = await client.kv.ttl(req.params.key);
        res.json({ key: req.params.key, ttl });
    } catch (err) {
        if (err instanceof NotFoundError) {
            res.status(404).json({
                key: req.params.key,
                ttl: null,
                error: "not_found",
            });
        } else {
            res.status(500).json({ key: req.params.key, error: err.message });
        }
    }
});

// GET /api/kv/list — List KV entries by prefix
app.get("/api/kv/list", async (req, res) => {
    try {
        const prefix = req.query.prefix || "";
        const entries = await client.kv.list(prefix);
        res.json({ prefix, entries });
    } catch (err) {
        res.status(500).json({ error: err.message });
    }
});

// POST /api/kv/batch-get — Batch get
app.post("/api/kv/batch-get", async (req, res) => {
    try {
        const { keys } = req.body;
        if (!Array.isArray(keys)) {
            return res.status(400).json({ error: "keys must be an array" });
        }
        const values = await client.kv.batch_get(keys);
        res.json({ keys, values });
    } catch (err) {
        res.status(500).json({ error: err.message });
    }
});

// POST /api/kv/batch-set — Batch set
app.post("/api/kv/batch-set", async (req, res) => {
    try {
        const pairs = req.body.pairs;
        if (!Array.isArray(pairs)) {
            return res.status(400).json({ error: "pairs must be an array" });
        }
        const result = await client.kv.batch_set(pairs);
        res.json({ count: pairs.length, result });
    } catch (err) {
        res.status(500).json({ error: err.message });
    }
});

// POST /api/kv/batch-delete — Batch delete
app.post("/api/kv/batch-delete", async (req, res) => {
    try {
        const { keys } = req.body;
        if (!Array.isArray(keys)) {
            return res.status(400).json({ error: "keys must be an array" });
        }
        const result = await client.kv.batch_delete(keys);
        res.json({ count: keys.length, result });
    } catch (err) {
        res.status(500).json({ error: err.message });
    }
});

// ─── Task 4: Settings Access ──────────────────────────────────────────────
app.get("/api/settings", async (_req, res) => {
    try {
        const settings = await client.settings.get(PLUGIN_SLUG);
        res.json({ slug: PLUGIN_SLUG, settings });
    } catch (err) {
        res.status(500).json({ slug: PLUGIN_SLUG, error: err.message });
    }
});

// ─── Task 5: Database Migrations ──────────────────────────────────────────
app.post("/api/migrate", async (_req, res) => {
    try {
        // List current migration status
        const status = await client.migrations.list(PLUGIN_SLUG);
        // Run pending migrations
        await client.migrations.run(PLUGIN_SLUG);
        res.json({
            slug: PLUGIN_SLUG,
            previous_status: status,
            message: "Migrations completed",
        });
    } catch (err) {
        res.status(500).json({ slug: PLUGIN_SLUG, error: err.message });
    }
});

// ─── Task 6: DB Query Demo ────────────────────────────────────────────────
app.get("/api/db/items", async (_req, res) => {
    try {
        const items = await client.db.query(
            PLUGIN_SLUG,
            "SELECT id, name, description, category, created_at FROM demo_items ORDER BY id ASC",
            [],
            30,
            100,
        );
        res.json({ slug: PLUGIN_SLUG, items });
    } catch (err) {
        res.status(500).json({ slug: PLUGIN_SLUG, error: err.message });
    }
});

// ─── Serve Vue pages (compiled) ───────────────────────────────────────────
app.get("/pages/dist/plugin-pages.js", async (_req, res) => {
    try {
        const { readFileSync } = await import("node:fs");
        const content = readFileSync("pages/dist/plugin-pages.js", "utf-8");
        res.type("application/javascript; charset=utf-8").send(content);
    } catch {
        res.status(404).json({
            error: "Pages not yet compiled — run npm run build:pages first",
        });
    }
});

// ─── Root ──────────────────────────────────────────────────────────────────
app.get("/", (_req, res) => {
    res.json({
        status: "ok",
        message: "Hello from Node.js plugin!",
        plugin: "hello-world-node",
        language: "Node.js / Express",
        endpoints: [
            "GET /health",
            "GET /api/hello",
            "GET|PUT|DELETE /api/kv/:key",
            "GET /api/kv/:key/ttl",
            "GET /api/kv/list?prefix=...",
            "POST /api/kv/batch-get",
            "POST /api/kv/batch-set",
            "POST /api/kv/batch-delete",
            "GET /api/settings",
            "POST /api/migrate",
            "GET /api/db/items",
        ],
    });
});

// ─── Start Server ─────────────────────────────────────────────────────────
app.listen(PORT, () => {
    console.log("=".repeat(60));
    console.log(`Hello-World-Node Plugin v1.0.0 starting up`);
    console.log(`Listening on 0.0.0.0:${PORT}`);
    console.log(`Core URL: ${CORE_URL}`);
    console.log(`Plugin Slug: ${PLUGIN_SLUG}`);
    console.log("=".repeat(60));
});
